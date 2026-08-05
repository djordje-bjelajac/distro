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

/// `alice`'s state, knowing `bob` (with a session) and `carol` (without one).
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
    assert_eq!(bob.last_seen_at, T0);
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
}

#[test]
fn presence_is_derived_at_read_time_from_the_clock() {
    let state = populated_state();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    let fresh: Vec<_> = handler
        .handle(ListKnownPeers)
        .iter()
        .map(|view| view.presence)
        .collect();
    assert_eq!(fresh, vec![Presence::Online, Presence::Online]);

    clock.advance(DurationMillis::from_secs(40));
    let stale: Vec<_> = handler
        .handle(ListKnownPeers)
        .iter()
        .map(|view| view.presence)
        .collect();
    assert_eq!(
        stale,
        vec![Presence::Stale, Presence::Stale],
        "the roster changed nothing; only the clock moved"
    );

    clock.advance(DurationMillis::from_secs(40));
    let offline: Vec<_> = handler
        .handle(ListKnownPeers)
        .iter()
        .map(|view| view.presence)
        .collect();
    assert_eq!(offline, vec![Presence::Offline, Presence::Offline]);
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
