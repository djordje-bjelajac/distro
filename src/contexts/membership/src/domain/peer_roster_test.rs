use shared_types::{PeerConnected, PeerDisconnected, PeerId};

use crate::domain::events::{PeerDiscovered, PeerPresenceExpired};
use crate::domain::{
    DurationMillis, Endpoint, KnownPeer, LivenessWindows, Millis, NetworkStatus, PeerRoster,
    PeerRosterError, Presence, SessionCollapse, SessionDirection, SessionState,
};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn later(millis: u64) -> Millis {
    T0.saturating_add(DurationMillis::from_millis(millis))
}

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

/// A roster local to `alice` that has been *told about* `peer` at `T0`.
///
/// Nothing has been heard from the peer itself, so it is `Unknown`: discovery
/// is a third party's claim, not evidence (D3). Tests that need a live peer
/// want [`roster_hearing_from`].
fn roster_knowing(peer: PeerId) -> PeerRoster {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    roster
        .record_discovery(
            peer,
            vec![endpoint("/ip4/198.51.100.7/udp/4001/quic-v1")],
            T0,
        )
        .expect("discovery of another peer is legal");
    roster
}

/// A roster local to `alice` that has *heard from* `peer` at `T0`.
///
/// The heartbeat is not decoration. Discovery leaves a peer `Unknown` forever,
/// so a test that wants an ageing presence has to supply real evidence — which
/// is what a frame arriving on a link with that peer produces.
fn roster_hearing_from(peer: PeerId) -> PeerRoster {
    let mut roster = roster_knowing(peer);
    roster
        .record_heartbeat(peer, T0)
        .expect("the peer was just discovered");
    roster
}

fn known(roster: &PeerRoster, peer: PeerId) -> &KnownPeer {
    roster.peer(&peer).expect("peer is in the roster")
}

// ---------------------------------------------------------------- discovery

#[test]
fn a_fresh_roster_knows_only_which_peer_it_is_local_to() {
    let roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(roster.local_peer(), test_peers::alice());
    assert!(roster.is_empty());
    assert_eq!(roster.len(), 0);
    assert_eq!(roster.established_session_count(), 0);
}

#[test]
fn recording_an_unknown_peer_adds_it_and_reports_the_discovery() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    let event = roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001")],
            T0,
        )
        .expect("legal discovery");

    assert_eq!(
        event,
        Some(PeerDiscovered {
            peer: test_peers::bob(),
            at: T0,
        })
    );
    let entry = known(&roster, test_peers::bob());
    assert_eq!(entry.peer(), test_peers::bob());
    assert_eq!(entry.endpoints(), [endpoint("/ip4/198.51.100.7/udp/4001")]);
    assert_eq!(
        entry.last_seen_at(),
        None,
        "we know where the peer claims to be, not that it is there"
    );
    assert_eq!(
        entry.recorded_at(),
        T0,
        "the instant is bookkeeping: when we wrote the entry down"
    );
    assert!(!entry.has_evidence());
    assert!(entry.session().is_none());
}

#[test]
fn rediscovering_a_known_peer_merges_endpoints_without_a_second_event() {
    // Discovery repeats constantly in a gossiping network; only the first is
    // news.
    let mut roster = roster_knowing(test_peers::bob());

    let event = roster
        .record_discovery(
            test_peers::bob(),
            vec![
                endpoint("/ip4/198.51.100.7/udp/4001/quic-v1"),
                endpoint("/ip6/2001:db8::1/udp/4001/quic-v1"),
            ],
            later(500),
        )
        .expect("legal rediscovery");

    assert_eq!(event, None);
    assert_eq!(roster.len(), 1);
    assert_eq!(
        known(&roster, test_peers::bob()).endpoints(),
        [
            endpoint("/ip4/198.51.100.7/udp/4001/quic-v1"),
            endpoint("/ip6/2001:db8::1/udp/4001/quic-v1"),
        ],
        "a repeated address is not stored twice"
    );
}

#[test]
fn discovery_is_not_evidence_of_life() {
    // The inverse of a test this file used to carry, which asserted the defect:
    // a discovery is something a *third party* said (invariant 2), so neither
    // the first sighting nor any later one may produce evidence of life. Both
    // halves matter — the constructor was one violation and the re-sighting
    // path was the other, and fixing either alone fixes nothing (S2).
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001")],
            T0,
        )
        .expect("legal discovery");
    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        None,
        "a first sighting is not evidence"
    );

    roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001")],
            later(9_000),
        )
        .expect("legal rediscovery");

    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        None,
        "and neither is a re-sighting"
    );
    assert_eq!(
        known(&roster, test_peers::bob()).presence(later(9_000), LivenessWindows::DEFAULT),
        Presence::Unknown
    );
}

#[test]
fn a_peer_only_ever_discovered_is_unknown_at_every_instant() {
    // Not Online at the instant it was recorded, and not sliding to Offline
    // later either: `Unknown` is the absence of a measurement, not a rung on
    // the ageing ladder, so nothing about the passage of time changes it.
    let windows = LivenessWindows::DEFAULT;
    let roster = roster_knowing(test_peers::bob());
    let entry = known(&roster, test_peers::bob());

    for now in [
        T0,
        later(1),
        later(30_000),
        later(60_000),
        later(u64::from(u32::MAX)),
    ] {
        assert_eq!(
            entry.presence(now, windows),
            Presence::Unknown,
            "peer heard from at no point is Unknown at {now}"
        );
    }
}

#[test]
fn a_never_heard_from_peer_is_unknown_and_not_offline() {
    // The two are different claims and are differently actionable: `Offline`
    // says "we were talking and they went away", `Unknown` says "we hold an
    // address and have never reached them — a dial is worth trying".
    let windows = LivenessWindows::DEFAULT;
    let roster = roster_knowing(test_peers::bob());
    let presence = known(&roster, test_peers::bob()).presence(later(600_000), windows);

    assert_eq!(presence, Presence::Unknown);
    assert_ne!(presence, Presence::Offline);
    assert!(
        !presence.is_offline(),
        "never heard from is not a departure"
    );
    assert!(!presence.is_online());
}

#[test]
fn rediscovering_a_never_heard_from_peer_leaves_it_unknown() {
    // A3b, and the vector that made this a security defect rather than a
    // cosmetic one: `record_discovery` is fed by `kad::Event::RoutingUpdated`
    // as well as mDNS, so a host publishing DHT records naming victim PeerIds
    // used to keep those victims permanently Online in every roster that
    // learned the record — refreshed on every re-announcement.
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_knowing(test_peers::bob());

    for announcement in 1..=20 {
        roster
            .record_discovery(
                test_peers::bob(),
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                later(announcement * 5_000),
            )
            .expect("legal re-announcement");

        assert_eq!(
            known(&roster, test_peers::bob()).presence(later(announcement * 5_000), windows),
            Presence::Unknown,
            "re-announcement {announcement} must not manufacture presence"
        );
    }

    assert_eq!(known(&roster, test_peers::bob()).last_seen_at(), None);
    assert_eq!(
        known(&roster, test_peers::bob()).recorded_at(),
        T0,
        "re-announcement does not reset the eviction clock either"
    );
}

#[test]
fn rediscovering_a_peer_that_has_evidence_does_not_refresh_it() {
    // The same vector against a peer we really have heard from: a flood of
    // announcements must not hold a departed peer at Online. Its presence goes
    // on ageing from the last thing the peer itself did.
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_hearing_from(test_peers::bob());

    roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001")],
            later(59_000),
        )
        .expect("legal re-announcement");

    let entry = known(&roster, test_peers::bob());
    assert_eq!(
        entry.last_seen_at(),
        Some(T0),
        "the evidence instant is still the heartbeat's"
    );
    assert_eq!(entry.presence(later(30_000), windows), Presence::Stale);
    assert_eq!(entry.presence(later(60_000), windows), Presence::Offline);
}

#[test]
fn the_roster_never_stores_the_local_peer() {
    // Invariant 2 at the aggregate boundary: a peer's own announcement comes
    // back from discovery, and it must not become a roster entry.
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    let rejected = roster.record_discovery(
        test_peers::alice(),
        vec![endpoint("/ip4/198.51.100.7/udp/4001")],
        T0,
    );

    assert_eq!(rejected, Err(PeerRosterError::SelfConnection));
    assert!(roster.is_empty());
    assert!(roster.peer(&test_peers::alice()).is_none());
}

#[test]
fn discovery_requires_at_least_one_endpoint() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.record_discovery(test_peers::bob(), Vec::new(), T0),
        Err(PeerRosterError::NoEndpoints)
    );
    assert!(roster.is_empty());
}

#[test]
fn the_endpoint_list_is_capped_and_drops_the_oldest_addresses_first() {
    // A peer legitimately has a handful of addresses; an unbounded list is a
    // cheap way for one peer to bloat every roster in the network.
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    let addresses: Vec<Endpoint> = (0..KnownPeer::MAX_ENDPOINTS + 2)
        .map(|index| endpoint(&format!("/ip4/198.51.100.{index}/udp/4001")))
        .collect();

    roster
        .record_discovery(test_peers::bob(), addresses.clone(), T0)
        .unwrap();

    let stored = known(&roster, test_peers::bob()).endpoints();
    assert_eq!(stored.len(), KnownPeer::MAX_ENDPOINTS);
    assert_eq!(stored, &addresses[2..], "the newest addresses are kept");
}

// ---------------------------------------------------------------- heartbeat

#[test]
fn a_heartbeat_refreshes_the_evidence_instant() {
    let mut roster = roster_hearing_from(test_peers::bob());

    assert_eq!(
        roster.record_heartbeat(test_peers::bob(), later(20_000)),
        Ok(())
    );

    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        Some(later(20_000))
    );
}

#[test]
fn the_first_heartbeat_takes_a_peer_out_of_unknown_and_it_ages_normally_from_there() {
    // The only exit from `Unknown` is evidence — and once there is some, the
    // ordinary ladder applies with no trace of where the peer started.
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_knowing(test_peers::bob());
    assert_eq!(
        known(&roster, test_peers::bob()).presence(T0, windows),
        Presence::Unknown
    );

    roster
        .record_heartbeat(test_peers::bob(), later(5_000))
        .expect("the peer is known");

    let entry = known(&roster, test_peers::bob());
    assert_eq!(entry.last_seen_at(), Some(later(5_000)));
    assert_eq!(entry.presence(later(5_000), windows), Presence::Online);
    assert_eq!(entry.presence(later(35_000), windows), Presence::Stale);
    assert_eq!(entry.presence(later(65_000), windows), Presence::Offline);

    assert_eq!(
        roster.expire_presence(later(65_000), windows),
        vec![PeerPresenceExpired {
            peer: test_peers::bob(),
            last_evidence_at: later(5_000),
            at: later(65_000),
        }],
        "a peer that has produced evidence can expire once it goes quiet"
    );
}

#[test]
fn a_heartbeat_from_an_unknown_peer_is_a_typed_error() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.record_heartbeat(test_peers::bob(), T0),
        Err(PeerRosterError::UnknownPeer)
    );
}

#[test]
fn a_heartbeat_claiming_the_local_peer_is_rejected() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.record_heartbeat(test_peers::alice(), T0),
        Err(PeerRosterError::SelfConnection)
    );
}

#[test]
fn evidence_never_moves_backwards() {
    // The clock behind ClockPort is monotonic (D11); if a reading regresses
    // anyway, a live peer must not be made to look stale by it.
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .record_heartbeat(test_peers::bob(), later(30_000))
        .unwrap();

    roster
        .record_heartbeat(test_peers::bob(), later(10_000))
        .unwrap();

    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        Some(later(30_000))
    );
}

// ----------------------------------------------------------------- presence

#[test]
fn presence_is_derived_from_the_evidence_the_roster_holds() {
    let windows = LivenessWindows::DEFAULT;
    let roster = roster_hearing_from(test_peers::bob());
    let entry = known(&roster, test_peers::bob());

    assert_eq!(entry.presence(T0, windows), Presence::Online);
    assert_eq!(entry.presence(later(30_000), windows), Presence::Stale);
    assert_eq!(entry.presence(later(60_000), windows), Presence::Offline);
}

#[test]
fn a_peer_that_was_never_heard_from_never_expires() {
    // Invariant 5: `PeerPresenceExpired` carries `last_evidence_at`, and for a
    // peer nothing ever arrived from there is no honest value to report — so
    // the event must not fire at all, at any age, over any number of sweeps.
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_knowing(test_peers::bob());

    for sweep in 1..=10_u64 {
        assert_eq!(
            roster.expire_presence(later(sweep * 120_000), windows),
            Vec::new(),
            "sweep {sweep} must report nothing for a peer that never spoke"
        );
    }

    assert!(
        roster.peer(&test_peers::bob()).is_some(),
        "and the peer stays known — it is still a dialable candidate"
    );
}

#[test]
fn expiring_presence_reports_each_peer_once_as_it_falls_offline() {
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_hearing_from(test_peers::bob());

    assert_eq!(roster.expire_presence(later(30_000), windows), Vec::new());

    let expired = roster.expire_presence(later(60_000), windows);
    assert_eq!(
        expired,
        vec![PeerPresenceExpired {
            peer: test_peers::bob(),
            last_evidence_at: T0,
            at: later(60_000),
        }]
    );

    assert_eq!(
        roster.expire_presence(later(90_000), windows),
        Vec::new(),
        "a peer that is already offline has not newly expired"
    );
}

#[test]
fn a_peer_can_expire_again_after_fresh_evidence() {
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_hearing_from(test_peers::bob());
    roster.expire_presence(later(60_000), windows);

    roster
        .record_heartbeat(test_peers::bob(), later(70_000))
        .unwrap();

    assert_eq!(
        roster.expire_presence(later(130_000), windows),
        vec![PeerPresenceExpired {
            peer: test_peers::bob(),
            last_evidence_at: later(70_000),
            at: later(130_000),
        }]
    );
}

#[test]
fn expiring_presence_reports_peers_in_a_deterministic_order() {
    let windows = LivenessWindows::DEFAULT;
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    for peer in [test_peers::carol(), test_peers::bob(), test_peers::dave()] {
        roster
            .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
            .unwrap();
        roster.record_heartbeat(peer, T0).unwrap();
    }

    let expired: Vec<_> = roster
        .expire_presence(later(60_000), windows)
        .into_iter()
        .map(|event| event.peer)
        .collect();

    assert_eq!(
        expired.len(),
        3,
        "all three peers had evidence, so the ordering assertion is not vacuous"
    );
    let mut sorted = expired.clone();
    sorted.sort_unstable();
    assert_eq!(expired, sorted, "iteration order follows PeerId order");
}

#[test]
fn expiring_presence_does_not_close_sessions() {
    // Presence and sessions are orthogonal: silence is not a close, and only
    // the transport can tell us a link is gone. The application decides
    // whether to act on an expiry.
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();
    roster.establish_session(test_peers::bob(), T0).unwrap();

    roster.expire_presence(later(600_000), windows);

    assert_eq!(
        known(&roster, test_peers::bob())
            .session()
            .map(|s| s.state()),
        Some(SessionState::Established)
    );
    assert_eq!(roster.established_session_count(), 1);
}

// ----------------------------------------------------------------- sessions

#[test]
fn opening_a_session_stores_it_as_connecting_and_publishes_nothing() {
    // `Connecting` is not connectivity: PeerConnected must not be published
    // before the handshake completes.
    let mut roster = roster_knowing(test_peers::bob());

    let outcome = roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .expect("legal open");

    assert_eq!(outcome.connected, None);
    assert_eq!(outcome.disconnected, None);
    assert_eq!(outcome.superseded, None);
    assert_eq!(outcome.collapse, None);
    assert_eq!(
        known(&roster, test_peers::bob())
            .session()
            .map(|s| s.state()),
        Some(SessionState::Connecting)
    );
    assert_eq!(roster.established_session_count(), 0);
}

#[test]
fn opening_a_session_to_the_local_peer_is_rejected() {
    // Invariant 2 at the roster level.
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.open_session(test_peers::alice(), SessionDirection::Inbound, T0),
        Err(PeerRosterError::SelfConnection)
    );
}

#[test]
fn opening_a_session_to_an_undiscovered_peer_is_rejected() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.open_session(test_peers::bob(), SessionDirection::Outbound, T0),
        Err(PeerRosterError::UnknownPeer)
    );
}

#[test]
fn a_second_session_in_the_same_direction_is_rejected() {
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();

    assert_eq!(
        roster.open_session(test_peers::bob(), SessionDirection::Outbound, later(1)),
        Err(PeerRosterError::SessionAlreadyOpen)
    );
}

#[test]
fn an_inbound_open_is_evidence_of_life_but_an_outbound_dial_is_not() {
    // A remote that dialled us has demonstrably just acted; our own dial
    // demonstrates nothing about them.
    let mut outbound_roster = roster_knowing(test_peers::bob());
    outbound_roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, later(5_000))
        .unwrap();
    assert_eq!(
        known(&outbound_roster, test_peers::bob()).last_seen_at(),
        None,
        "neither the discovery nor our own dial says anything about the peer"
    );

    let mut inbound_roster = roster_knowing(test_peers::bob());
    inbound_roster
        .open_session(test_peers::bob(), SessionDirection::Inbound, later(5_000))
        .unwrap();
    assert_eq!(
        known(&inbound_roster, test_peers::bob()).last_seen_at(),
        Some(later(5_000)),
        "a remote that dialled us has demonstrably just acted"
    );
}

#[test]
fn establishing_a_session_publishes_peer_connected_and_refreshes_evidence() {
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();

    let outcome = roster
        .establish_session(test_peers::bob(), later(2_000))
        .expect("legal establish");

    assert_eq!(
        outcome.connected,
        Some(PeerConnected {
            peer: test_peers::bob()
        })
    );
    assert_eq!(outcome.disconnected, None);
    assert_eq!(roster.established_session_count(), 1);
    assert!(known(&roster, test_peers::bob()).is_connected());
    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        Some(later(2_000)),
        "a completed handshake is proof of life"
    );
}

#[test]
fn establishing_without_a_session_is_rejected() {
    let mut roster = roster_knowing(test_peers::bob());

    assert_eq!(
        roster.establish_session(test_peers::bob(), T0),
        Err(PeerRosterError::NoSession)
    );
}

#[test]
fn establishing_twice_is_rejected_so_peer_connected_is_published_once() {
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();
    roster.establish_session(test_peers::bob(), T0).unwrap();

    assert_eq!(
        roster.establish_session(test_peers::bob(), later(1)),
        Err(PeerRosterError::InvalidSessionTransition {
            from: SessionState::Established,
            to: SessionState::Established,
        })
    );
}

#[test]
fn closing_an_established_session_publishes_peer_disconnected() {
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();
    roster.establish_session(test_peers::bob(), T0).unwrap();

    let outcome = roster
        .close_session(test_peers::bob())
        .expect("legal close");

    assert_eq!(
        outcome.disconnected,
        Some(PeerDisconnected {
            peer: test_peers::bob()
        })
    );
    assert_eq!(outcome.connected, None);
    assert_eq!(roster.established_session_count(), 0);
    assert!(
        known(&roster, test_peers::bob()).session().is_none(),
        "the peer stays known; only the link is gone"
    );
}

#[test]
fn closing_a_session_that_never_established_publishes_nothing() {
    // No PeerConnected was ever published for it, so an unmatched
    // PeerDisconnected would make `messaging` fail directs for a peer it never
    // considered reachable (D10).
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();

    let outcome = roster
        .close_session(test_peers::bob())
        .expect("legal close");

    assert_eq!(outcome.disconnected, None);
    assert_eq!(outcome.connected, None);
}

#[test]
fn closing_without_a_session_is_rejected() {
    let mut roster = roster_knowing(test_peers::bob());

    assert_eq!(
        roster.close_session(test_peers::bob()),
        Err(PeerRosterError::NoSession)
    );
}

// ------------------------------------------------------- simultaneous connect

#[test]
fn simultaneous_connect_keeps_the_local_session_when_the_local_peer_is_lower() {
    // dave (0x27…) < alice (0xd7…): dave's dial survives, so alice's roster
    // keeps its inbound session and discards its own outbound one.
    let local = test_peers::alice();
    let remote = test_peers::dave();
    assert!(remote < local, "fixture ordering");

    let mut roster = PeerRoster::for_local_peer(local);
    roster
        .record_discovery(remote, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
        .unwrap();
    roster
        .open_session(remote, SessionDirection::Inbound, T0)
        .unwrap();

    let outcome = roster
        .open_session(remote, SessionDirection::Outbound, later(10))
        .expect("a simultaneous connect is a normal case, not an error");

    assert_eq!(
        outcome.collapse,
        Some(SessionCollapse::resolve(local, remote).unwrap())
    );
    assert_eq!(
        outcome.superseded,
        Some(SessionDirection::Outbound),
        "the session we just opened is the one to abandon"
    );
    assert_eq!(outcome.connected, None);
    assert_eq!(outcome.disconnected, None);
    assert_eq!(
        known(&roster, remote).session().map(|s| s.direction()),
        Some(SessionDirection::Inbound),
        "the stored session is unchanged"
    );
}

#[test]
fn simultaneous_connect_replaces_the_local_session_when_the_remote_peer_is_lower() {
    // bob (0x3d…) < alice (0xd7…) as well, so bob's inbound dial supersedes
    // alice's outbound one — the mirror of the previous case.
    let local = test_peers::alice();
    let remote = test_peers::bob();
    assert!(remote < local, "fixture ordering");

    let mut roster = roster_knowing(remote);
    roster
        .open_session(remote, SessionDirection::Outbound, T0)
        .unwrap();

    let outcome = roster
        .open_session(remote, SessionDirection::Inbound, later(10))
        .expect("legal collapse");

    assert_eq!(outcome.superseded, Some(SessionDirection::Outbound));
    assert_eq!(
        known(&roster, remote).session().map(|s| s.direction()),
        Some(SessionDirection::Inbound),
        "the surviving inbound session replaces the stored one"
    );
    assert_eq!(
        known(&roster, remote).session().map(|s| s.state()),
        Some(SessionState::Connecting)
    );
}

#[test]
fn both_sides_of_a_simultaneous_connect_keep_the_same_wire_session() {
    // The symmetry that matters end to end: run the same race on two rosters,
    // one per peer, and check they agree without exchanging anything.
    let peers = test_peers::all();
    for (index, local) in peers.iter().enumerate() {
        for remote in &peers[index + 1..] {
            let ours = collapse_outcome(*local, *remote);
            let theirs = collapse_outcome(*remote, *local);

            let our_survivor = ours.survivor_direction();
            let their_survivor = theirs.survivor_direction();
            assert_eq!(
                our_survivor,
                their_survivor.opposite(),
                "both sides must keep the same wire session"
            );
            assert_eq!(
                our_survivor.initiator(*local, *remote),
                their_survivor.initiator(*remote, *local),
                "and agree on who initiated it"
            );
        }
    }
}

/// Drives one roster through a simultaneous connect and reports the outcome of
/// the second open.
fn collapse_outcome(local: PeerId, remote: PeerId) -> CollapseSummary {
    let mut roster = PeerRoster::for_local_peer(local);
    roster
        .record_discovery(remote, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
        .unwrap();
    roster
        .open_session(remote, SessionDirection::Outbound, T0)
        .unwrap();
    let outcome = roster
        .open_session(remote, SessionDirection::Inbound, later(10))
        .expect("legal collapse");

    CollapseSummary {
        stored: known(&roster, remote)
            .session()
            .expect("a session survives")
            .direction(),
        superseded: outcome.superseded.expect("one session is discarded"),
    }
}

struct CollapseSummary {
    stored: SessionDirection,
    superseded: SessionDirection,
}

impl CollapseSummary {
    fn survivor_direction(&self) -> SessionDirection {
        assert_eq!(
            self.stored,
            self.superseded.opposite(),
            "the roster must keep exactly the session it did not supersede"
        );
        self.stored
    }
}

#[test]
fn superseding_an_established_session_publishes_peer_disconnected() {
    // The established link really is going away, and the replacement has not
    // handshaked yet, so reachability genuinely drops for that interval.
    let local = test_peers::alice();
    let remote = test_peers::bob();
    let mut roster = roster_knowing(remote);
    roster
        .open_session(remote, SessionDirection::Outbound, T0)
        .unwrap();
    roster.establish_session(remote, T0).unwrap();

    let outcome = roster
        .open_session(remote, SessionDirection::Inbound, later(10))
        .expect("legal collapse");

    assert_eq!(outcome.superseded, Some(SessionDirection::Outbound));
    assert_eq!(
        outcome.disconnected,
        Some(PeerDisconnected { peer: remote }),
        "the established link is gone and messaging must hear about it"
    );
    assert_eq!(outcome.connected, None);
    assert_eq!(roster.established_session_count(), 0);
    assert_eq!(
        SessionCollapse::resolve(local, remote).unwrap().survivor(),
        SessionDirection::Inbound
    );
}

#[test]
fn a_closed_session_does_not_collapse_with_a_new_one() {
    // Reconnecting after a drop is an ordinary open, not a simultaneous
    // connect.
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();
    roster.close_session(test_peers::bob()).unwrap();

    let outcome = roster
        .open_session(test_peers::bob(), SessionDirection::Inbound, later(10))
        .expect("legal open");

    assert_eq!(outcome.collapse, None);
    assert_eq!(outcome.superseded, None);
}

// ----------------------------------------------------------------- roster cap

/// Fills a roster to one short of the cap with peers nothing was ever heard
/// from, recorded oldest-first, and reports them in that order.
fn roster_filled_with_unproven_peers(count: usize) -> (PeerRoster, Vec<PeerId>) {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    let peers = test_peers::synthetic(count);

    for (index, peer) in peers.iter().enumerate() {
        roster
            .record_discovery(
                *peer,
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                later(index as u64),
            )
            .expect("filling the roster below the cap is legal");
    }

    (roster, peers)
}

#[test]
fn the_roster_cap_evicts_the_oldest_never_heard_from_peer_first() {
    // Never-heard-from peers do not expire, so without a cap the roster is a
    // permanent leak that anyone publishing DHT records can grow for free
    // (D9, S5).
    let (mut roster, unproven) = roster_filled_with_unproven_peers(PeerRoster::MAX_PEERS);
    assert_eq!(roster.len(), PeerRoster::MAX_PEERS);

    let newcomer = test_peers::bob();
    roster
        .record_discovery(
            newcomer,
            vec![endpoint("/ip4/198.51.100.9/udp/4001")],
            later(1_000_000),
        )
        .expect("a discovery at the cap makes room rather than failing");

    assert_eq!(roster.len(), PeerRoster::MAX_PEERS, "the cap holds");
    assert!(
        roster.peer(&unproven[0]).is_none(),
        "the entry that sat unproven longest goes first"
    );
    assert!(
        roster.peer(&unproven[1]).is_some(),
        "and only as many as needed are evicted"
    );
    assert!(roster.peer(&newcomer).is_some());
}

#[test]
fn the_roster_cap_never_evicts_a_peer_with_a_session_or_with_evidence() {
    // The peers this roster has actually reached are the ones worth keeping.
    // Evicting one to make room for an unproven identity would let a flood of
    // announcements displace the working network — the failure the cap exists
    // to prevent.
    let (mut roster, unproven) = roster_filled_with_unproven_peers(PeerRoster::MAX_PEERS - 2);

    let with_evidence = test_peers::bob();
    let with_session = test_peers::carol();
    // Both are recorded *after* every unproven peer, so an eviction rule that
    // only looked at age would take them first.
    roster
        .record_discovery(
            with_evidence,
            vec![endpoint("/ip4/198.51.100.8/udp/4001")],
            later(900_000),
        )
        .unwrap();
    roster
        .record_heartbeat(with_evidence, later(900_000))
        .unwrap();
    roster
        .record_discovery(
            with_session,
            vec![endpoint("/ip4/198.51.100.9/udp/4001")],
            later(900_001),
        )
        .unwrap();
    roster
        .open_session(with_session, SessionDirection::Outbound, later(900_001))
        .unwrap();
    assert_eq!(roster.len(), PeerRoster::MAX_PEERS);

    roster
        .record_discovery(
            test_peers::dave(),
            vec![endpoint("/ip4/198.51.100.10/udp/4001")],
            later(1_000_000),
        )
        .expect("legal discovery at the cap");

    assert!(
        roster.peer(&with_evidence).is_some(),
        "a peer that produced evidence is never evicted"
    );
    assert!(
        roster.peer(&with_session).is_some(),
        "nor is one holding a session, even an unestablished one"
    );
    assert!(
        roster.peer(&unproven[0]).is_none(),
        "the oldest unproven entry is what made room"
    );
    assert_eq!(roster.len(), PeerRoster::MAX_PEERS);
}

#[test]
fn a_roster_full_of_reached_peers_refuses_a_new_discovery() {
    // Nothing is evictable, so the honest answer is a typed refusal: silently
    // dropping the discovery would be indistinguishable from recording it, and
    // evicting a real peer is forbidden.
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    for (index, peer) in test_peers::synthetic(PeerRoster::MAX_PEERS)
        .into_iter()
        .enumerate()
    {
        roster
            .record_discovery(
                peer,
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                later(index as u64),
            )
            .unwrap();
        roster.record_heartbeat(peer, later(index as u64)).unwrap();
    }

    assert_eq!(
        roster.record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.9/udp/4001")],
            later(1_000_000),
        ),
        Err(PeerRosterError::RosterFull)
    );
    assert_eq!(roster.len(), PeerRoster::MAX_PEERS);
}

#[test]
fn rediscovering_a_known_peer_at_the_cap_is_not_an_insertion() {
    // A re-announcement of a peer already held must not be refused for want of
    // room: it adds no entry.
    let (mut roster, unproven) = roster_filled_with_unproven_peers(PeerRoster::MAX_PEERS);

    assert_eq!(
        roster.record_discovery(
            unproven[0],
            vec![endpoint("/ip6/2001:db8::1/udp/4001/quic-v1")],
            later(1_000_000),
        ),
        Ok(None)
    );
    assert_eq!(roster.len(), PeerRoster::MAX_PEERS);
    assert!(roster.peer(&unproven[0]).is_some());
}

// ------------------------------------------------------------------- removal

#[test]
fn removing_a_connected_peer_forgets_it_and_reports_the_disconnect() {
    let mut roster = roster_knowing(test_peers::bob());
    roster
        .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
        .unwrap();
    roster.establish_session(test_peers::bob(), T0).unwrap();

    let disconnected = roster.remove(test_peers::bob()).expect("legal removal");

    assert_eq!(
        disconnected,
        Some(PeerDisconnected {
            peer: test_peers::bob()
        })
    );
    assert!(roster.is_empty());
    assert_eq!(roster.established_session_count(), 0);
}

#[test]
fn removing_a_peer_that_was_never_connected_reports_no_disconnect() {
    let mut roster = roster_knowing(test_peers::bob());

    assert_eq!(roster.remove(test_peers::bob()), Ok(None));
    assert!(roster.is_empty());
}

#[test]
fn removing_an_unknown_peer_is_a_typed_error() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    assert_eq!(
        roster.remove(test_peers::bob()),
        Err(PeerRosterError::UnknownPeer)
    );
}

// -------------------------------------------------------------------- status

#[test]
fn established_sessions_drive_the_network_status() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    assert_eq!(
        NetworkStatus::from_connected_peers(roster.established_session_count()),
        NetworkStatus::Isolated
    );

    for peer in [test_peers::bob(), test_peers::carol()] {
        roster
            .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
            .unwrap();
        roster
            .open_session(peer, SessionDirection::Outbound, T0)
            .unwrap();
        roster.establish_session(peer, T0).unwrap();
    }

    assert_eq!(
        NetworkStatus::from_connected_peers(roster.established_session_count()),
        NetworkStatus::from_connected_peers(2)
    );
}

#[test]
fn known_peers_are_listed_in_peer_id_order() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    for peer in [test_peers::carol(), test_peers::bob(), test_peers::erin()] {
        roster
            .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
            .unwrap();
    }

    let listed: Vec<PeerId> = roster.known_peers().map(KnownPeer::peer).collect();

    assert_eq!(
        listed,
        vec![test_peers::bob(), test_peers::erin(), test_peers::carol()],
        "0x3d… < 0xec… < 0xfc…"
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            PeerRosterError::SelfConnection,
            "the local peer is never a roster entry",
        ),
        (PeerRosterError::UnknownPeer, "peer is not in the roster"),
        (
            PeerRosterError::NoEndpoints,
            "a discovered peer must carry at least one endpoint",
        ),
        (
            PeerRosterError::RosterFull,
            "the roster is full of peers that have sessions or evidence",
        ),
        (
            PeerRosterError::NoSession,
            "peer has no live session in the roster",
        ),
        (
            PeerRosterError::SessionAlreadyOpen,
            "a live session in that direction already exists for the peer",
        ),
        (
            PeerRosterError::InvalidSessionTransition {
                from: SessionState::Closed,
                to: SessionState::Established,
            },
            "session cannot move from closed to established",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
