use crate::domain::{Endpoint, KnownPeer, Millis, PeerRoster};
use crate::ports::port_fakes::{InMemoryPeerCache, UnusablePeerCache};
use crate::ports::{CachedPeer, PeerCacheError, PeerCachePort};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn cached_bob() -> CachedPeer {
    CachedPeer {
        peer: test_peers::bob(),
        endpoints: vec![endpoint("/ip4/198.51.100.7/udp/4001/quic-v1")],
        last_seen_at: T0,
    }
}

#[test]
fn an_empty_cache_is_a_cold_start_not_an_error() {
    // D1's first rung: a fresh install has no cached peers, which is exactly
    // the case the ladder exists for.
    let cache = InMemoryPeerCache::empty();
    let port: &dyn PeerCachePort = &cache;

    assert_eq!(port.load(), Ok(Vec::new()));
}

#[test]
fn saved_peers_come_back_on_the_next_launch() {
    let cache = InMemoryPeerCache::empty();
    let port: &dyn PeerCachePort = &cache;

    assert_eq!(port.save(&[cached_bob()]), Ok(()));

    assert_eq!(port.load(), Ok(vec![cached_bob()]));
}

#[test]
fn saving_replaces_the_stored_set_rather_than_appending() {
    // The roster is the whole truth about known peers; a cache that only ever
    // grew could never forget a peer the user removed.
    let cache = InMemoryPeerCache::holding(vec![cached_bob()]);
    let port: &dyn PeerCachePort = &cache;

    port.save(&[]).unwrap();

    assert_eq!(port.load(), Ok(Vec::new()));
}

#[test]
fn a_cache_entry_is_built_from_a_roster_entry() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001/quic-v1")],
            T0,
        )
        .unwrap();
    let entry: &KnownPeer = roster.peer(&test_peers::bob()).unwrap();

    assert_eq!(CachedPeer::of(entry), cached_bob());
}

#[test]
fn a_cache_entry_carries_no_session_state() {
    // Sessions do not survive a process; caching one would make a peer look
    // connected on the next launch before anything was dialled.
    let cached = cached_bob();

    assert_eq!(cached.peer, test_peers::bob());
    assert_eq!(cached.last_seen_at, T0);
    assert_eq!(cached.endpoints.len(), 1);
}

#[test]
fn a_foreign_schema_version_is_a_typed_error_not_a_destructive_rewrite() {
    // S4: an unknown version must be surfaced with the original preserved.
    let cache = UnusablePeerCache(PeerCacheError::UnsupportedSchemaVersion { found: 2 });
    let port: &dyn PeerCachePort = &cache;

    assert_eq!(
        port.load(),
        Err(PeerCacheError::UnsupportedSchemaVersion { found: 2 })
    );
    assert_eq!(
        port.save(&[cached_bob()]),
        Err(PeerCacheError::UnsupportedSchemaVersion { found: 2 })
    );
}

#[test]
fn every_failure_is_typed_rather_than_a_panic() {
    let failures = [
        PeerCacheError::Unreadable,
        PeerCacheError::Corrupt,
        PeerCacheError::UnsupportedSchemaVersion { found: 7 },
        PeerCacheError::WriteFailed,
    ];

    for failure in failures {
        let cache = UnusablePeerCache(failure);
        let port: &dyn PeerCachePort = &cache;

        assert_eq!(port.load(), Err(failure));
    }
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (PeerCacheError::Unreadable, "peer cache could not be read"),
        (
            PeerCacheError::Corrupt,
            "peer cache does not contain a usable peer set",
        ),
        (
            PeerCacheError::UnsupportedSchemaVersion { found: 7 },
            "peer cache has unsupported schema version 7",
        ),
        (
            PeerCacheError::WriteFailed,
            "peer cache could not be written",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
