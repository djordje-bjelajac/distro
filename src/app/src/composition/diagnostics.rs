use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use infra_net_libp2p::swarm::Reachability;

/// The local counters AC6, AC14 and AC15 ask for, plus the one non-counted fact
/// the interface reports the same way — whether strangers can dial this peer.
///
/// # Why counters and not log lines
///
/// Three acceptance criteria require that something be *counted in local
/// diagnostics*: envelopes that failed signature verification (AC6), envelopes
/// carrying an unknown minor addition (AC14), and abandoned gaps (AC15). A log
/// line satisfies none of them on a terminal application whose whole screen is
/// the conversation — the user would have to leave the program to see it. These
/// are numbers a pane can show.
///
/// Every counter is a plain `AtomicU64` with relaxed ordering: they are
/// reported, never compared against each other, and no decision anywhere reads
/// one. `infra-net-libp2p` keeps its own (`CodecDiagnostics`) for what only it
/// can see — oversize frames, rate-limited peers, dropped events — and the UI
/// shows both.
///
/// # The one thing here that is not a number
///
/// [`Reachability`] is a state, not a tally, and it lives here because it is
/// reported exactly like one: the root receives it, keeps the latest, and shows
/// it. It is *held and nothing more* — no dial, no relay reservation and no
/// address selection changes on the strength of it (reachability canvas D4,
/// S5) — so this stays a place where facts are recorded rather than acted on.
#[derive(Debug, Default)]
pub struct Diagnostics {
    envelopes_accepted: AtomicU64,
    envelopes_refused: AtomicU64,
    envelopes_ignored: AtomicU64,
    duplicates_ignored: AtomicU64,
    gaps_abandoned: AtomicU64,
    messages_never_received: AtomicU64,
    heartbeats_sent: AtomicU64,
    heartbeats_failed: AtomicU64,
    direct_delivery_failures: AtomicU64,
    uncorrelated_reports: AtomicU64,
    port_refusals: AtomicU64,
    /// The last answer the network gave to "can strangers dial this peer".
    ///
    /// No `Option` wrapper: [`Reachability::Unknown`] *is* the honest default,
    /// and `Option<Reachability>` would give the same fact two spellings — one
    /// of which every call site would have to remember not to render as
    /// alarming.
    reachability: Mutex<Reachability>,
}

macro_rules! counter {
    ($count:ident, $read:ident) => {
        pub fn $count(&self) {
            self.$read.fetch_add(1, Ordering::Relaxed);
        }

        pub fn $read(&self) -> u64 {
            self.$read.load(Ordering::Relaxed)
        }
    };
}

impl Diagnostics {
    counter!(count_envelope_accepted, envelopes_accepted);
    counter!(count_envelope_refused, envelopes_refused);
    counter!(count_envelope_ignored, envelopes_ignored);
    counter!(count_duplicate_ignored, duplicates_ignored);
    counter!(count_heartbeat_sent, heartbeats_sent);
    counter!(count_heartbeat_failed, heartbeats_failed);
    counter!(count_direct_delivery_failure, direct_delivery_failures);
    counter!(count_uncorrelated_report, uncorrelated_reports);
    counter!(count_port_refusal, port_refusals);

    /// Records one abandoned gap and how many of an author's messages it wrote
    /// off (AC15).
    ///
    /// Two numbers because they answer different questions: how often the
    /// network is losing runs, and how much was lost. One gap of forty is a
    /// different fault from forty gaps of one.
    pub fn count_gap_abandoned(&self, messages: u64) {
        self.gaps_abandoned.fetch_add(1, Ordering::Relaxed);
        self.messages_never_received
            .fetch_add(messages, Ordering::Relaxed);
    }

    pub fn gaps_abandoned(&self) -> u64 {
        self.gaps_abandoned.load(Ordering::Relaxed)
    }

    pub fn messages_never_received(&self) -> u64 {
        self.messages_never_received.load(Ordering::Relaxed)
    }

    /// Records the verdict the network just reported.
    ///
    /// Replaces rather than accumulates, and needs no debouncing:
    /// `NetworkEvent::ReachabilityChanged` is emitted only on an actual
    /// transition, so whatever last arrived is what holds now — including the
    /// return to [`Reachability::Reachable`] after a failure, which the adapter
    /// deliberately does not latch.
    pub fn record_reachability(&self, reachability: Reachability) {
        *self
            .reachability
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = reachability;
    }

    /// The verdict as it stands.
    ///
    /// [`Reachability::Unknown`] until a probe concludes — which is a different
    /// fact from [`Reachability::Unreachable`] and must stay one everywhere it
    /// is read (reachability canvas S3).
    pub fn reachability(&self) -> Reachability {
        self.reachability
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
