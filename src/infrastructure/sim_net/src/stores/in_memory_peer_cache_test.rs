use membership::domain::{Endpoint, Millis};
use membership::ports::{CachedPeer, PeerCacheError, PeerCachePort};
use shared_types::PeerId;

use crate::crypto::SimKeypair;
use crate::stores::InMemoryPeerCache;

fn bob() -> PeerId {
    SimKeypair::derived(1, "bob").peer()
}

fn cached(peer: PeerId, address: &str) -> CachedPeer {
    CachedPeer {
        peer,
        endpoints: vec![Endpoint::direct(address).expect("a valid address")],
        last_seen_at: Millis::from_millis(10),
    }
}

#[test]
fn a_cold_start_loads_empty_rather_than_failing() {
    // The empty cache is exactly the case the rest of the bootstrap ladder
    // exists for; it is not an error.
    let cache = InMemoryPeerCache::empty();

    assert_eq!(cache.load(), Ok(Vec::new()));
}

#[test]
fn a_warm_cache_offers_its_peers_as_bootstrap_candidates() {
    let cache = InMemoryPeerCache::warm(vec![cached(bob(), "sim://bob")]);

    assert!(cache.holds(bob()));
    assert_eq!(cache.load().expect("healthy cache").len(), 1);
}

#[test]
fn saving_replaces_rather_than_merges() {
    // The roster is the whole truth about known peers; an append-only cache
    // could never forget one.
    let cache = InMemoryPeerCache::warm(vec![cached(bob(), "sim://bob")]);

    cache.save(&[]).expect("healthy cache");

    assert_eq!(cache.load(), Ok(Vec::new()));
    assert_eq!(cache.save_count(), 1);
}

#[test]
fn an_injected_read_fault_costs_a_rung_and_nothing_else() {
    let cache = InMemoryPeerCache::warm(vec![cached(bob(), "sim://bob")]);
    cache.fail_loads_with(PeerCacheError::Corrupt);

    assert_eq!(cache.load(), Err(PeerCacheError::Corrupt));

    cache.repair();
    assert!(cache.load().expect("repaired cache").len() == 1);
}

#[test]
fn an_injected_write_fault_leaves_the_stored_set_untouched() {
    let cache = InMemoryPeerCache::warm(vec![cached(bob(), "sim://bob")]);
    cache.fail_saves_with(PeerCacheError::WriteFailed);

    assert_eq!(cache.save(&[]), Err(PeerCacheError::WriteFailed));
    assert!(cache.holds(bob()));
    assert_eq!(cache.save_count(), 0);
}

#[test]
fn seeding_arranges_a_warm_start_without_simulating_one_first() {
    let cache = InMemoryPeerCache::empty();

    cache.seed(vec![cached(bob(), "sim://bob")]);

    assert!(cache.holds(bob()));
}
