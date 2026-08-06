use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::queries::{ListKnownPeers, ListKnownPeersHandler};
use crate::domain::{
    DurationMillis, Endpoint, LivenessWindows, Millis, Presence, SessionDirection, SessionState,
};
use crate::ports::ClockPort;
use crate::ports::port_fakes::ManualClock;
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

/// `alice`'s state, knowing `bob` (who completed a handshake, and so has both a
/// session and evidence of life at `T0`) and `carol` (who was named by a
/// discovery and has done nothing since).
fn populated_state() -> Arc<MembershipState> {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        roster
            .record_discovery(
                test_peers::bob(),
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                T0,
            )
            .expect("discovery");
        roster
            .record_discovery(
                test_peers::carol(),
                vec![endpoint("/ip4/203.0.113.9/udp/4001")],
                T0,
            )
            .expect("discovery");
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
        roster
            .establish_session(test_peers::bob(), T0)
            .expect("establish");
    });
    state
}

fn handler_over(state: &Arc<MembershipState>, clock: &Arc<ManualClock>) -> ListKnownPeersHandler {
    ListKnownPeersHandler::new(
        Arc::clone(state),
        Arc::clone(clock) as Arc<dyn ClockPort + Send + Sync>,
        LivenessWindows::DEFAULT,
    )
}

#[test]
fn every_known_peer_is_reported_in_peer_id_order() {
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));

    let peers = handler_over(&state, &clock).handle(ListKnownPeers);

    let ids: Vec<_> = peers.iter().map(|view| view.peer).collect();
    let mut expected = vec![test_peers::bob(), test_peers::carol()];
    expected.sort_unstable();
    assert_eq!(ids, expected, "a stable order keeps a redraw deterministic");
}

#[test]
fn a_view_carries_the_endpoints_session_and_evidence_the_ui_renders() {
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));

    let peers = handler_over(&state, &clock).handle(ListKnownPeers);
    let bob = peers
        .iter()
        .find(|view| view.peer == test_peers::bob())
        .expect("bob is known");

    assert_eq!(bob.endpoints, vec![endpoint("/ip4/198.51.100.7/udp/4001")]);
    assert_eq!(bob.session, Some(SessionState::Established));
    assert!(bob.is_connected());
    assert_eq!(
        bob.last_seen_at,
        Some(T0),
        "the handshake instant, which is the one thing bob actually did"
    );
}

#[test]
fn a_peer_without_a_session_is_known_but_not_connected() {
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));

    let peers = handler_over(&state, &clock).handle(ListKnownPeers);
    let carol = peers
        .iter()
        .find(|view| view.peer == test_peers::carol())
        .expect("carol is known");

    assert_eq!(carol.session, None);
    assert!(
        !carol.is_connected(),
        "discovery says where a peer is, never that it is reachable"
    );
    assert_eq!(
        carol.last_seen_at, None,
        "and the instant we were told about her is not an instant she was seen at"
    );
    assert_eq!(carol.presence, Presence::Unknown);
}

#[test]
fn presence_is_derived_at_read_time_from_the_clock() {
    // The ladder is an ageing of one measurement, so it needs a peer that
    // produced one. This used to assert that carol — discovered, never heard
    // from — walked Online → Stale → Offline alongside bob, which is the defect:
    // it aged an instant she never produced. Both halves are asserted now, and
    // the second is the stronger claim: the clock moves bob down the ladder and
    // cannot put carol on it at all (canvas D1, invariant 4).
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    let presence_of = |peer| {
        handler
            .handle(ListKnownPeers)
            .into_iter()
            .find(|view| view.peer == peer)
            .expect("peer is known")
            .presence
    };

    assert_eq!(presence_of(test_peers::bob()), Presence::Online);
    assert_eq!(presence_of(test_peers::carol()), Presence::Unknown);

    clock.advance(DurationMillis::from_secs(40));
    assert_eq!(
        presence_of(test_peers::bob()),
        Presence::Stale,
        "the roster changed nothing; only the clock moved"
    );
    assert_eq!(presence_of(test_peers::carol()), Presence::Unknown);

    clock.advance(DurationMillis::from_secs(40));
    assert_eq!(presence_of(test_peers::bob()), Presence::Offline);
    assert_eq!(
        presence_of(test_peers::carol()),
        Presence::Unknown,
        "Unknown is not a rung on the ladder, so no amount of time walks off it"
    );
}

#[test]
fn reading_the_roster_leaves_it_untouched() {
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    let before = state.read(Clone::clone);
    for _ in 0..5 {
        clock.advance(DurationMillis::from_secs(30));
        let _ = handler.handle(ListKnownPeers);
    }
    let after = state.read(Clone::clone);

    assert_eq!(
        before, after,
        "a query path that mutated would make presence a fact someone set (invariant 7)"
    );
}
