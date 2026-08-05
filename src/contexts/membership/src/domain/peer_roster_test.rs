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

/// A roster local to `alice`, already knowing `peer` as of `T0`.
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
    assert_eq!(entry.last_seen_at(), T0);
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
fn discovery_is_evidence_of_life() {
    let mut roster = roster_knowing(test_peers::bob());

    roster
        .record_discovery(
            test_peers::bob(),
            vec![endpoint("/ip4/198.51.100.7/udp/4001")],
            later(9_000),
        )
        .unwrap();

    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        later(9_000)
    );
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
    let mut roster = roster_knowing(test_peers::bob());

    assert_eq!(
        roster.record_heartbeat(test_peers::bob(), later(20_000)),
        Ok(())
    );

    assert_eq!(
        known(&roster, test_peers::bob()).last_seen_at(),
        later(20_000)
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
        later(30_000)
    );
}

// ----------------------------------------------------------------- presence

#[test]
fn presence_is_derived_from_the_evidence_the_roster_holds() {
    let windows = LivenessWindows::DEFAULT;
    let roster = roster_knowing(test_peers::bob());
    let entry = known(&roster, test_peers::bob());

    assert_eq!(entry.presence(T0, windows), Presence::Online);
    assert_eq!(entry.presence(later(30_000), windows), Presence::Stale);
    assert_eq!(entry.presence(later(60_000), windows), Presence::Offline);
}

#[test]
fn expiring_presence_reports_each_peer_once_as_it_falls_offline() {
    let windows = LivenessWindows::DEFAULT;
    let mut roster = roster_knowing(test_peers::bob());

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
    let mut roster = roster_knowing(test_peers::bob());
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
    }

    let expired: Vec<_> = roster
        .expire_presence(later(60_000), windows)
        .into_iter()
        .map(|event| event.peer)
        .collect();

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
        T0
    );

    let mut inbound_roster = roster_knowing(test_peers::bob());
    inbound_roster
        .open_session(test_peers::bob(), SessionDirection::Inbound, later(5_000))
        .unwrap();
    assert_eq!(
        known(&inbound_roster, test_peers::bob()).last_seen_at(),
        later(5_000)
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
        later(2_000),
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
