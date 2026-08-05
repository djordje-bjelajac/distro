use std::sync::Arc;

use shared_types::PeerId;

use crate::crypto::SimKeypair;
use crate::stores::{InMemoryPeerCache, InMemoryTrustRecords, PersistentSequenceCounter};

/// Everything about a simulated peer that outlives its process.
///
/// # This type is D12
///
/// A restart replaces a peer's three contexts and its message log; it does
/// *not* replace this. The split is not an implementation convenience — it is
/// the exact condition AC16 pins:
///
/// > *A peer that restarts continues to be heard by peers already online — its
/// > outbound sequence does not reset.*
///
/// With in-memory-only history (D7) a restarted peer used to resume at
/// sequence 1 while every peer still online held its high-water mark at N, so
/// each message it sent was correctly classified a duplicate and ignored: the
/// peer went permanently mute while appearing, to itself, to work. Keeping the
/// counter here, beside the keypair whose lifetime it shares, is what fixes it —
/// and putting the message log on the *other* side of the line is what stops a
/// scenario from proving it by accident.
///
/// # What each member is, and why it survives
///
/// * [`keypair`](Self::keypair) — the identity itself. AC9: a `PeerId` is
///   stable across restarts.
/// * [`counter`](Self::counter) — the outbound sequence, with the keypair's
///   lifetime exactly (D12).
/// * [`trust`](Self::trust) — a verification performed once must not have to be
///   repeated, and a blocked peer stays blocked.
/// * [`cache`](Self::cache) — the warm start that makes a join ticket a
///   one-time cost (D1).
///
/// Conversation history is deliberately absent: it dies with the process (D7).
pub struct DurablePeerState {
    keypair: Arc<SimKeypair>,
    counter: Arc<PersistentSequenceCounter>,
    trust: Arc<InMemoryTrustRecords>,
    cache: Arc<InMemoryPeerCache>,
}

impl DurablePeerState {
    /// A brand-new machine: a fresh identity, an empty cache, no trust records,
    /// and a counter that has issued nothing.
    pub fn fresh(seed: u64, label: &str) -> Self {
        Self {
            keypair: Arc::new(SimKeypair::derived(seed, label)),
            counter: Arc::new(PersistentSequenceCounter::fresh()),
            trust: Arc::new(InMemoryTrustRecords::empty()),
            cache: Arc::new(InMemoryPeerCache::empty()),
        }
    }

    /// This machine's stable identity (AC9).
    pub fn peer(&self) -> PeerId {
        self.keypair.peer()
    }

    /// The keypair, for wiring the key store and the signer to one key.
    pub fn keypair(&self) -> &Arc<SimKeypair> {
        &self.keypair
    }

    /// The outbound sequence counter (D12, AC16).
    pub fn counter(&self) -> &Arc<PersistentSequenceCounter> {
        &self.counter
    }

    /// The verification and block state, which `messaging`'s author policy also
    /// reads (invariant 11).
    pub fn trust(&self) -> &Arc<InMemoryTrustRecords> {
        &self.trust
    }

    /// The warm-start peer cache (D1).
    pub fn cache(&self) -> &Arc<InMemoryPeerCache> {
        &self.cache
    }
}
