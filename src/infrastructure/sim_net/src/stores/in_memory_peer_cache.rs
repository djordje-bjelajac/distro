use std::sync::Mutex;

use membership::ports::{CachedPeer, PeerCacheError, PeerCachePort};
use shared_types::PeerId;

use crate::stores::guard;

/// The warm-start peer cache, in memory (D1, rung (a) of the bootstrap ladder).
///
/// # It outlives the process, because that is the whole point
///
/// The cache is what makes a join ticket a one-time cost: after a first
/// successful join, a machine bootstraps from what it already knows. The
/// harness therefore keeps this store *outside* a peer's contexts and hands the
/// same instance to every rebuild, so a restarted peer finds its cache exactly
/// as `infra-store-fs` will make it find its file (OP-11).
///
/// # Faults are injectable
///
/// A cache that cannot be read costs a rung and is reported in the join
/// diagnostic; one that cannot be written costs a warm start and is reported in
/// the leave outcome. Both are stated outcomes rather than errors, and AC3 is
/// about the diagnostic being visible — so this store can be told to fail on
/// purpose.
#[derive(Debug, Default)]
pub struct InMemoryPeerCache {
    state: Mutex<CacheState>,
}

#[derive(Debug, Default)]
struct CacheState {
    peers: Vec<CachedPeer>,
    saves: usize,
    load_fault: Option<PeerCacheError>,
    save_fault: Option<PeerCacheError>,
}

impl InMemoryPeerCache {
    /// An empty cache: the cold start every fresh install has.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A cache already holding `peers`, as if a previous session had left them.
    pub fn warm(peers: Vec<CachedPeer>) -> Self {
        let cache = Self::default();
        guard(&cache.state).peers = peers;
        cache
    }

    /// Replaces the cached set without going through the port, so a scenario
    /// can arrange a warm start it did not have to simulate first.
    pub fn seed(&self, peers: Vec<CachedPeer>) {
        guard(&self.state).peers = peers;
    }

    /// What the cache currently holds.
    pub fn peers(&self) -> Vec<CachedPeer> {
        guard(&self.state).peers.clone()
    }

    /// Whether `peer` would be a bootstrap candidate on the next launch.
    pub fn holds(&self, peer: PeerId) -> bool {
        guard(&self.state)
            .peers
            .iter()
            .any(|cached| cached.peer == peer)
    }

    /// How many times the cache has been written.
    pub fn save_count(&self) -> usize {
        guard(&self.state).saves
    }

    /// Makes every read fail with `error`, until [`repair`](Self::repair).
    pub fn fail_loads_with(&self, error: PeerCacheError) {
        guard(&self.state).load_fault = Some(error);
    }

    /// Makes every write fail with `error`, until [`repair`](Self::repair).
    pub fn fail_saves_with(&self, error: PeerCacheError) {
        guard(&self.state).save_fault = Some(error);
    }

    /// Clears any injected fault.
    pub fn repair(&self) {
        let mut state = guard(&self.state);
        state.load_fault = None;
        state.save_fault = None;
    }
}

impl PeerCachePort for InMemoryPeerCache {
    fn load(&self) -> Result<Vec<CachedPeer>, PeerCacheError> {
        let state = guard(&self.state);

        match state.load_fault {
            Some(error) => Err(error),
            None => Ok(state.peers.clone()),
        }
    }

    fn save(&self, peers: &[CachedPeer]) -> Result<(), PeerCacheError> {
        let mut state = guard(&self.state);

        if let Some(error) = state.save_fault {
            return Err(error);
        }

        // Replace rather than merge, exactly as the port requires: the roster
        // is the whole truth about known peers, and an append-only cache could
        // never forget one.
        state.peers = peers.to_vec();
        state.saves += 1;
        Ok(())
    }
}
