use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use shared_types::EnvelopeSignature;

/// The signatures this peer released as heartbeats, so a report about one is
/// never mistaken for a report about a message (canvas `0010` §7/S6).
///
/// # Why heartbeats need a correlation of their own
///
/// Since D7 a heartbeat travels as a **direct** message, which means the
/// transport answers for it exactly as it answers for a real one:
/// `DirectMessageDelivered` when the recipient takes it in,
/// `DirectMessageFailed` when nothing comes back. Those two events name a
/// signature, and [`DeliveryIndex`](crate::composition::DeliveryIndex) is the
/// only thing that could previously have recognised one.
///
/// A heartbeat is not in that index and must never be: it carries no
/// `MessageId`, belongs to no conversation, and has no delivery state to move.
/// So without this ledger every heartbeat report would land in the index's
/// "there is no message this could name" branch — which, on the failure side,
/// raises the user-visible notice *"a message to X was not delivered"*. Once
/// per presence tick. Every ten seconds, for as long as a peer stays
/// unreachable, about a message the user never sent.
///
/// That is the whole reason this type exists, and it is why the check must run
/// **before** the index is consulted rather than as a fallback after it.
///
/// # The lookup does not consume, and that is the difference from `DeliveryIndex`
///
/// [`DeliveryIndex::take`](crate::composition::DeliveryIndex::take) removes an
/// entry because a message is answered at most once. A heartbeat is the
/// opposite: one round signs **one** envelope and sends that same envelope to
/// every linked peer, so one signature attracts one report *per peer*.
/// Consuming it would let the first peer's answer be recognised and turn every
/// other peer's into the notice above — the same defect, merely rarer and
/// harder to reproduce.
///
/// # Bounded, and in this build the bound is never approached
///
/// A heartbeat envelope is fully determined by four things that do not change
/// while a process runs: the protocol version, [`PayloadKind::Heartbeat`], this
/// peer's identity, and an empty payload. Ed25519 signing is deterministic, so
/// every heartbeat this instance ever sends carries the **same** signature and
/// this set holds exactly one member for the life of the process.
///
/// It is capped anyway. That property is a consequence of what a heartbeat
/// happens to carry today, not a rule anything enforces, and a bound costs
/// nothing to keep: were a heartbeat ever to gain varying content, an unbounded
/// set here would become a slow leak on a ten-second timer instead of a
/// compile-time question.
#[derive(Debug)]
pub struct HeartbeatLedger {
    entries: Mutex<Signatures>,
    capacity: usize,
}

#[derive(Debug, Default)]
struct Signatures {
    held: HashSet<EnvelopeSignature>,
    order: VecDeque<EnvelopeSignature>,
}

impl HeartbeatLedger {
    /// Distinct heartbeat signatures remembered at once.
    ///
    /// One is enough for this build (see above). Eight leaves room for a
    /// heartbeat that someday varies without making the cap something a caller
    /// has to think about.
    pub const DEFAULT_CAPACITY: usize = 8;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(Signatures::default()),
            capacity: capacity.max(1),
        }
    }

    /// Remembers that `signature` was released as a heartbeat.
    ///
    /// Idempotent: recording the same signature again — which is what every
    /// round after the first does — neither duplicates it nor pushes anything
    /// out.
    pub fn record(&self, signature: EnvelopeSignature) {
        let mut entries = self.lock();

        if !entries.held.insert(signature) {
            return;
        }

        entries.order.push_back(signature);

        while entries.order.len() > self.capacity {
            if let Some(oldest) = entries.order.pop_front() {
                entries.held.remove(&oldest);
            }
        }
    }

    /// Whether a delivery report naming `signature` is about a heartbeat.
    ///
    /// Answering `true` means the report has no message behind it and no notice
    /// to raise. Answering `false` means the ordinary message path applies, so
    /// a signature this has never seen is treated exactly as it was before this
    /// type existed.
    pub fn is_heartbeat(&self, signature: &EnvelopeSignature) -> bool {
        self.lock().held.contains(signature)
    }

    /// How many distinct heartbeat signatures are remembered.
    pub fn held(&self) -> usize {
        self.lock().held.len()
    }

    fn lock(&self) -> MutexGuard<'_, Signatures> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for HeartbeatLedger {
    fn default() -> Self {
        Self::new()
    }
}
