//! The retention rule, the bound, and the eviction order — none of which can
//! be driven through the driver, because the driver's clock is
//! `Instant::elapsed` and a test cannot wait fifteen minutes.
//!
//! Time is an argument here, so ageing is asserted at both edges rather than
//! approximately, and the same sequence of sightings produces the same contents
//! on every run. What *can* be driven through the driver — that a second read
//! answers the same thing as the first, and that the bound holds under a flood
//! arriving as real mDNS events — is asserted there as well, in
//! `network_driver_test.rs`.

use std::time::Duration;

use membership::domain::{Endpoint, KnownPeer, Reachability};
use shared_types::PeerId;

use crate::swarm::sighting_ledger::{SIGHTING_RETENTION, SightingLedger};
use crate::test_peers;

/// The shipped retention window in milliseconds, which is what the ledger
/// counts in.
fn retention_millis() -> u64 {
    u64::try_from(SIGHTING_RETENTION.as_millis()).expect("fifteen minutes fits in a u64")
}

fn endpoint(text: &str) -> Endpoint {
    Endpoint::new(text, Reachability::Direct).expect("a well-formed address")
}

fn address(index: usize) -> Endpoint {
    endpoint(&format!("/ip4/192.168.1.10/tcp/{}", 4001 + index))
}

/// A ledger with the shipped retention window and a capacity a test can fill.
fn ledger(max_peers: usize) -> SightingLedger {
    SightingLedger::new(max_peers, SIGHTING_RETENTION)
}

fn peers_reported(ledger: &mut SightingLedger, now: u64) -> Vec<PeerId> {
    ledger
        .observed(now)
        .into_iter()
        .map(|sighting| sighting.peer)
        .collect()
}

#[test]
fn reading_the_ledger_leaves_it_exactly_as_it_was() {
    // **D12 and A7, at the level that decides them.** The defect was a
    // destructive drain, so the assertion is not "the second read is
    // non-empty" but "the second read is the *same*" — and the third too, so
    // that an implementation answering twice and then forgetting would still
    // fail.
    let peers = test_peers::synthetic(2);
    let mut ledger = ledger(8);
    ledger.record(peers[0], &[address(0)], 0);
    ledger.record(peers[1], &[address(1)], 0);

    let first = ledger.observed(1_000);
    let second = ledger.observed(2_000);
    let third = ledger.observed(3_000);

    assert_eq!(first.len(), 2);
    assert_eq!(first, second, "reading is a question, not a withdrawal");
    assert_eq!(second, third);
    assert_eq!(ledger.len(), 2, "and nothing left the ledger");
}

#[test]
fn a_sighting_survives_to_the_end_of_its_window_and_not_past_it() {
    // The retention rule, at both edges. Asserting only the far side would
    // pass for a ledger that expired everything immediately, which is the
    // destructive drain wearing a different hat.
    let peer = test_peers::synthetic(1)[0];
    let retention = retention_millis();
    let mut ledger = ledger(8);
    ledger.record(peer, &[address(0)], 0);

    assert_eq!(
        peers_reported(&mut ledger, retention - 1),
        vec![peer],
        "still inside the window"
    );
    assert!(
        peers_reported(&mut ledger, retention).is_empty(),
        "and gone at the end of it"
    );
    assert_eq!(
        ledger.len(),
        0,
        "dropped, not merely filtered out of a read"
    );
}

#[test]
fn a_peer_that_is_still_announcing_never_ages_out() {
    // The window runs from the *last* sighting, which is what makes it a
    // freshness rule rather than a lifetime. A LAN neighbour re-announces every
    // mDNS round, so it must be able to sit in the ledger for many multiples of
    // the window while it is genuinely there.
    let peer = test_peers::synthetic(1)[0];
    let round = retention_millis() / 3;
    let mut ledger = ledger(8);

    for tick in 0..12 {
        ledger.record(peer, &[address(0)], tick * round);
        assert_eq!(peers_reported(&mut ledger, tick * round), vec![peer]);
    }

    assert_eq!(ledger.len(), 1, "refreshed, never duplicated");
}

#[test]
fn ageing_out_happens_on_a_read_as_well_as_on_a_write() {
    // A process that joins once and then only listens must not hold a stale
    // sighting forever, and neither must one that discovers and never joins.
    // Both paths expire, so neither is a way to keep the ledger from ageing.
    let peers = test_peers::synthetic(2);
    let stale = retention_millis() + 1;
    let mut ledger = ledger(8);
    ledger.record(peers[0], &[address(0)], 0);

    // Read only: the stale entry is gone even though nothing was recorded.
    assert!(peers_reported(&mut ledger, stale).is_empty());
    assert_eq!(ledger.len(), 0);

    // Write only: recording a *different* peer much later expires the first.
    ledger.record(peers[0], &[address(0)], 0);
    ledger.record(peers[1], &[address(1)], stale);
    assert_eq!(ledger.len(), 1);
    assert_eq!(peers_reported(&mut ledger, stale), vec![peers[1]]);
}

#[test]
fn the_bound_holds_under_a_flood_of_sightings() {
    // **Canvas §7/S6.** Sightings are attacker-influenceable: mDNS is
    // answerable by any host on the link and a Kademlia routing update by any
    // peer that can place a record in the DHT, so identities are free. Now that
    // the read no longer empties the buffer, this cap and the retention window
    // are the only two things between that and unbounded growth.
    let peers = test_peers::synthetic(1_000);
    let mut ledger = ledger(4);

    for (index, peer) in peers.iter().enumerate() {
        ledger.record(*peer, &[address(index)], index as u64);
        assert!(
            ledger.len() <= 4,
            "the cap holds at every step, not only at the end"
        );
    }

    assert_eq!(ledger.len(), 4);
    assert_eq!(ledger.observed(1_000).len(), 4);
}

#[test]
fn the_least_recently_seen_sighting_is_the_one_evicted() {
    // The eviction order, which is a stated rule and not an accident of the
    // container: freshness is the only thing this type knows, and the peer that
    // announced itself most recently is the one most likely to answer a dial.
    let peers = test_peers::synthetic(5);
    let mut ledger = ledger(3);
    for (index, peer) in peers.iter().take(3).enumerate() {
        ledger.record(*peer, &[address(index)], (index as u64 + 1) * 100);
    }

    // The first peer is the stalest, so it goes when a fourth arrives.
    ledger.record(peers[3], &[address(3)], 400);
    assert_eq!(
        sorted(peers_reported(&mut ledger, 400)),
        sorted(vec![peers[1], peers[2], peers[3]])
    );

    // Re-announcing rescues an entry: the second peer is refreshed, so the
    // third is now the stalest and is what the next arrival displaces.
    ledger.record(peers[1], &[address(1)], 500);
    ledger.record(peers[4], &[address(4)], 600);
    assert_eq!(
        sorted(peers_reported(&mut ledger, 600)),
        sorted(vec![peers[1], peers[3], peers[4]])
    );
}

#[test]
fn a_full_ledger_evicts_the_same_entry_every_time_it_is_replayed() {
    // Determinism, including the tie-break: several peers seen in the same
    // millisecond are ordered by identity, so a flood arriving in one batch does
    // not evict differently on a machine with a different hash seed.
    let peers = test_peers::synthetic(5);
    let outcome = || {
        let mut ledger = ledger(2);
        for index in [3_usize, 1, 4, 0, 2] {
            ledger.record(peers[index], &[address(index)], 42);
        }
        peers_reported(&mut ledger, 42)
    };

    let first = outcome();
    assert_eq!(first.len(), 2);
    for _ in 0..8 {
        assert_eq!(outcome(), first);
    }
}

#[test]
fn one_peer_announcing_endlessly_cannot_grow_its_own_sighting() {
    // The other half of the bound. Capping the number of peers would achieve
    // nothing if a single peer could report ten thousand addresses, and the
    // addresses arrive from the network exactly as the identities do. The cap
    // is the domain's own, so this buffer never holds an address the roster
    // would discard anyway.
    let peer = test_peers::synthetic(1)[0];
    let mut ledger = ledger(8);

    for index in 0..2_000 {
        ledger.record(peer, &[address(index)], index as u64);
    }

    let observed = ledger.observed(2_000);
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].endpoints.len(), KnownPeer::MAX_ENDPOINTS);
    assert_eq!(
        observed[0].endpoints.last(),
        Some(&address(1_999)),
        "newest wins: the address a peer just claimed is the one worth keeping"
    );
}

#[test]
fn a_re_sighting_merges_endpoints_rather_than_replacing_or_duplicating_them() {
    // A peer is seen on several transports and by several mechanisms — mDNS
    // reports the LAN address, Kademlia the public one — and a sighting that
    // dropped whichever arrived second would hand the ladder one fewer thing to
    // try than the network actually offered.
    let peer = test_peers::synthetic(1)[0];
    let lan = endpoint("/ip4/192.168.1.10/tcp/4001");
    let public = endpoint("/ip4/203.0.113.7/tcp/4001");
    let mut ledger = ledger(8);

    ledger.record(peer, std::slice::from_ref(&lan), 0);
    ledger.record(peer, std::slice::from_ref(&public), 10);
    ledger.record(peer, std::slice::from_ref(&lan), 20);

    let observed = ledger.observed(30);
    assert_eq!(observed.len(), 1);
    assert_eq!(
        observed[0].endpoints,
        vec![lan, public],
        "both addresses, each once, in the order they were claimed"
    );
}

#[test]
fn a_ledger_nobody_feeds_reports_nothing_and_says_so_by_being_empty() {
    // An empty read is success, not failure — a LAN with no neighbour is the
    // ordinary state of a first launch — and it stays success long after the
    // window has passed with nothing in it.
    let mut ledger = ledger(8);

    assert!(ledger.observed(0).is_empty());
    assert!(ledger.observed(retention_millis() * 100).is_empty());
    assert_eq!(ledger.len(), 0);
}

#[test]
fn a_ledger_that_may_hold_nothing_holds_nothing_rather_than_looping() {
    // A degenerate capacity is a configuration mistake, not a hang. Asserted
    // because the eviction loop's exit condition is the kind of thing that is
    // correct for every sensible value and non-terminating for one.
    let peer = test_peers::synthetic(1)[0];
    let mut ledger = SightingLedger::new(0, SIGHTING_RETENTION);

    ledger.record(peer, &[address(0)], 0);

    assert_eq!(ledger.len(), 0);
    assert!(ledger.observed(0).is_empty());
}

#[test]
fn a_retention_window_of_zero_keeps_nothing() {
    // The mirror of the case above, and the reason the window is a `<`
    // comparison and not a `<=`: a ledger told to remember nothing must
    // remember nothing, not one sighting.
    let peer = test_peers::synthetic(1)[0];
    let mut ledger = SightingLedger::new(8, Duration::ZERO);

    ledger.record(peer, &[address(0)], 0);

    assert!(ledger.observed(0).is_empty());
    assert_eq!(ledger.len(), 0);
}

/// Synthetic peers come in insertion order, not `PeerId` order, so a set
/// comparison has to sort both sides rather than assume either.
fn sorted(mut peers: Vec<PeerId>) -> Vec<PeerId> {
    peers.sort_unstable();
    peers
}
