use std::fs;
use std::path::PathBuf;

use membership::domain::{Endpoint, Millis, Reachability};
use membership::ports::{CachedPeer, PeerCacheError, PeerCachePort};
use shared_types::PeerId;

use crate::format::hex_bytes;
use crate::stores::FilePeerCache;
use crate::test_dir::TestDir;
use crate::test_peers::{alice, bob};

fn cache(dir: &TestDir) -> FilePeerCache {
    FilePeerCache::at(dir.file(FilePeerCache::FILE_NAME))
}

fn plant(dir: &TestDir, contents: &str) -> PathBuf {
    let path = dir.file(FilePeerCache::FILE_NAME);
    fs::write(&path, contents).expect("the plant must land");
    path
}

fn cached(peer: PeerId, addresses: &[(&str, Reachability)], last_seen_at: u64) -> CachedPeer {
    CachedPeer {
        peer,
        endpoints: addresses
            .iter()
            .map(|(address, reachability)| {
                Endpoint::new(address, *reachability).expect("a valid fixture address")
            })
            .collect(),
        last_seen_at: Millis::from_millis(last_seen_at),
    }
}

#[test]
fn a_cold_start_loads_an_empty_cache_rather_than_failing() {
    let dir = TestDir::new("cache-cold-start");

    // Exactly the case the rest of the bootstrap ladder exists for (D1).
    assert_eq!(cache(&dir).load(), Ok(Vec::new()));
}

#[test]
fn endpoints_and_reachability_round_trip() {
    let dir = TestDir::new("cache-round-trip");
    let cache = cache(&dir);

    let peers = vec![
        cached(
            alice(),
            &[
                ("/ip4/198.51.100.7/udp/4001/quic-v1", Reachability::Direct),
                (
                    "/ip4/203.0.113.9/udp/4001/quic-v1/p2p-circuit",
                    Reachability::Relayed,
                ),
            ],
            1_234,
        ),
        cached(
            bob(),
            &[("/ip6/2001:db8::1/udp/4001/quic-v1", Reachability::Direct)],
            5_678,
        ),
    ];

    cache.save(&peers).expect("the save must land");

    assert_eq!(cache.load(), Ok(peers));
}

#[test]
fn a_peer_with_no_endpoints_round_trips() {
    let dir = TestDir::new("cache-no-endpoints");
    let cache = cache(&dir);
    let peers = vec![cached(alice(), &[], 42)];

    cache.save(&peers).expect("the save must land");

    assert_eq!(cache.load(), Ok(peers));
}

#[test]
fn an_address_containing_spaces_round_trips() {
    let dir = TestDir::new("cache-spaces");
    let cache = cache(&dir);
    // The domain only forbids control characters, so a space is legal in an
    // address — which is why the address is the last field on its line.
    let peers = vec![cached(
        alice(),
        &[("/dns4/a host with spaces/tcp/1", Reachability::Direct)],
        1,
    )];

    cache.save(&peers).expect("the save must land");

    assert_eq!(cache.load(), Ok(peers));
}

#[test]
fn save_replaces_rather_than_merges() {
    let dir = TestDir::new("cache-replace");
    let cache = cache(&dir);

    cache
        .save(&[cached(
            alice(),
            &[("/ip4/198.51.100.7/udp/1/quic-v1", Reachability::Direct)],
            1,
        )])
        .expect("the save must land");
    cache
        .save(&[cached(
            bob(),
            &[("/ip4/198.51.100.8/udp/2/quic-v1", Reachability::Direct)],
            2,
        )])
        .expect("the save must land");

    // An append-only cache could never forget a peer, which is why the port
    // says replace.
    let loaded = cache.load().expect("the load must succeed");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].peer, bob());
}

#[test]
fn an_emptied_cache_stays_empty() {
    let dir = TestDir::new("cache-emptied");
    let cache = cache(&dir);

    cache
        .save(&[cached(
            alice(),
            &[("/ip4/198.51.100.7/udp/1/quic-v1", Reachability::Direct)],
            1,
        )])
        .expect("the save must land");
    cache.save(&[]).expect("the save must land");

    assert_eq!(cache.load(), Ok(Vec::new()));
}

/// What forgetting every peer leaves behind, asserted as bytes.
///
/// The round-trip above proves the value comes back empty; this proves the
/// *file* is empty, which is the thing a user who forgot their peers actually
/// asked for. A format change that started writing a placeholder line, or that
/// left the old peers in place and marked them somehow, would still pass a
/// round-trip and would still hand the next launch a warm start nobody wanted.
#[test]
fn an_emptied_cache_holds_a_header_and_nothing_else() {
    let dir = TestDir::new("cache-emptied-file");
    let writer = cache(&dir);
    writer
        .save(&[cached(
            alice(),
            &[("/ip4/198.51.100.7/udp/1/quic-v1", Reachability::Direct)],
            1,
        )])
        .expect("the save must land");

    writer.save(&[]).expect("the save must land");

    let contents = fs::read_to_string(dir.file(FilePeerCache::FILE_NAME)).expect("the file exists");
    assert_eq!(
        contents.lines().count(),
        1,
        "the schema header, and no peer line: {contents:?}"
    );
    assert!(!contents.contains(&hex_bytes::encode(alice().as_bytes())));
    // And a fresh reader agrees — the emptiness survives the process that
    // caused it, which is the whole point of writing it down.
    assert_eq!(cache(&dir).load(), Ok(Vec::new()));
}

#[test]
fn the_cache_survives_a_restart() {
    let dir = TestDir::new("cache-restart");
    let peers = vec![cached(
        alice(),
        &[("/ip4/198.51.100.7/udp/1/quic-v1", Reachability::Direct)],
        9,
    )];

    cache(&dir).save(&peers).expect("the save must land");

    // The warm start is what makes a join ticket a one-time cost.
    assert_eq!(cache(&dir).load(), Ok(peers));
}

#[test]
fn the_saved_order_is_preserved() {
    let dir = TestDir::new("cache-order");
    let cache = cache(&dir);
    let peers = vec![cached(bob(), &[], 2), cached(alice(), &[], 1)];

    cache.save(&peers).expect("the save must land");

    // The roster decides which peers to try first; a cache that re-sorted them
    // would be overriding the bootstrap ladder.
    assert_eq!(cache.load(), Ok(peers));
}

#[test]
fn an_endpoint_count_larger_than_the_endpoints_present_is_corrupt() {
    let dir = TestDir::new("cache-short-count");
    let path = plant(
        &dir,
        &format!(
            "distro-peer-cache 1\npeer {} 1 2\nendpoint direct /ip4/198.51.100.7/udp/1/quic-v1\n",
            hex_bytes::encode(alice().as_bytes())
        ),
    );

    // A truncated file must not read as a shorter valid one.
    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::Corrupt)
    );
}

#[test]
fn an_absurd_endpoint_count_is_corrupt_rather_than_an_allocation() {
    let dir = TestDir::new("cache-absurd-count");
    let path = plant(
        &dir,
        &format!(
            "distro-peer-cache 1\npeer {} 1 {}\n",
            hex_bytes::encode(alice().as_bytes()),
            u64::MAX
        ),
    );

    // Regression: the declared count used to size the endpoint vector before a
    // single endpoint had been read, so a two-line file could ask for an
    // allocation the process aborts on. A count is a claim, not a measurement
    // (S6: caps hold before the data is trusted).
    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::Corrupt)
    );
}

#[test]
fn an_endpoint_line_without_a_peer_line_is_corrupt() {
    let dir = TestDir::new("cache-orphan-endpoint");
    let path = plant(
        &dir,
        "distro-peer-cache 1\nendpoint direct /ip4/198.51.100.7/udp/1/quic-v1\n",
    );

    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::Corrupt)
    );
}

#[test]
fn an_address_the_domain_would_reject_is_corrupt() {
    let dir = TestDir::new("cache-bad-address");
    let path = plant(
        &dir,
        &format!(
            "distro-peer-cache 1\npeer {} 1 1\nendpoint direct \u{7}bell\n",
            hex_bytes::encode(alice().as_bytes())
        ),
    );

    // A file is input too: an `Endpoint` that skipped its own constructor would
    // be a domain value nobody checked.
    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::Corrupt)
    );
}

#[test]
fn a_non_numeric_last_seen_is_corrupt_rather_than_a_panic() {
    let dir = TestDir::new("cache-bad-instant");
    let path = plant(
        &dir,
        &format!(
            "distro-peer-cache 1\npeer {} yesterday 0\n",
            hex_bytes::encode(alice().as_bytes())
        ),
    );

    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::Corrupt)
    );
}

#[test]
fn an_unknown_schema_version_is_reported_and_the_cache_is_preserved() {
    let dir = TestDir::new("cache-future-version");
    let original = "distro-peer-cache 4\nwhatever a later build writes\n";
    let path = plant(&dir, original);

    assert_eq!(
        FilePeerCache::at(&path).load(),
        Err(PeerCacheError::UnsupportedSchemaVersion { found: 4 })
    );
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_save_that_cannot_land_reports_write_failed() {
    let dir = TestDir::new("cache-write-fails");
    let cache = FilePeerCache::at(dir.file("never-created").join(FilePeerCache::FILE_NAME));

    assert_eq!(cache.save(&[]), Err(PeerCacheError::WriteFailed));
}

#[test]
fn a_half_written_temp_file_is_never_the_cache() {
    let dir = TestDir::new("cache-stale-temp");
    let cache = cache(&dir);
    let peers = vec![cached(alice(), &[], 1)];

    cache.save(&peers).expect("the save must land");
    fs::write(
        dir.file(&format!("{}.tmp-424242-0", FilePeerCache::FILE_NAME)),
        "distro-peer-cache 1\npeer half-writ",
    )
    .expect("the plant must land");

    assert_eq!(cache.load(), Ok(peers));
}
