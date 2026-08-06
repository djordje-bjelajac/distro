use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::commands::{RecordPeerHeartbeat, RecordPeerHeartbeatHandler};
use crate::domain::{
    DurationMillis, Endpoint, LivenessWindows, Millis, PeerRosterError, Presence, SessionDirection,
    SessionState,
};
use crate::ports::port_fakes::ManualClock;
use crate::ports::{ClockPort, MembershipCommandError};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

/// `alice`'s state, holding an address for `bob` and nothing else: a peer she
/// was told about, which has produced no evidence of life.
fn state_knowing_bob() -> Arc<MembershipState> {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        roster
            .record_discovery(
                test_peers::bob(),
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                T0,
            )
            .expect("discovery");
    });
    state
}

/// The same, after `bob` has actually spoken at `T0`.
fn state_having_heard_bob() -> Arc<MembershipState> {
    let state = state_knowing_bob();
    state.modify(|roster| {
        roster
            .record_heartbeat(test_peers::bob(), T0)
            .expect("bob speaks");
    });
    state
}

fn presence_of_bob(state: &Arc<MembershipState>, now: Millis) -> Option<Presence> {
    state.read(|roster| {
        roster
            .peer(&test_peers::bob())
            .map(|entry| entry.presence(now, LivenessWindows::DEFAULT))
    })
}

fn handler_over(
    state: &Arc<MembershipState>,
    clock: &Arc<ManualClock>,
) -> RecordPeerHeartbeatHandler {
    RecordPeerHeartbeatHandler::new(
        Arc::clone(state),
        Arc::clone(clock) as Arc<dyn ClockPort + Send + Sync>,
    )
}

#[test]
fn a_heartbeat_refreshes_the_evidence_of_life() {
    let state = state_knowing_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    clock.advance(DurationMillis::from_secs(20));

    handler_over(&state, &clock)
        .handle(RecordPeerHeartbeat {
            peer: test_peers::bob(),
        })
        .expect("bob is known");

    assert_eq!(
        state.read(|roster| roster.peer(&test_peers::bob()).map(|e| e.last_seen_at())),
        Some(Some(T0.saturating_add(DurationMillis::from_secs(20)))),
        "the heartbeat created the evidence; nothing before it had"
    );
}

#[test]
fn only_a_heartbeat_takes_a_peer_out_of_unknown_however_long_it_has_been_known() {
    // The inversion of what `a_heartbeat_pulls_a_stale_peer_back_to_online` used
    // to assert about its starting state. Its fixture reached `Stale` by ageing
    // the *discovery* instant, which is the defect: a peer that has told us
    // nothing is not two-thirds of the way to offline, it is off the ladder
    // entirely, and it stays there at every age (invariant 4, canvas D1).
    let state = state_knowing_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    for _ in 0..10 {
        clock.advance(DurationMillis::from_secs(30));
        assert_eq!(
            presence_of_bob(&state, clock.now()),
            Some(Presence::Unknown),
            "ageing cannot manufacture a rung for a peer that never spoke"
        );
    }

    handler
        .handle(RecordPeerHeartbeat {
            peer: test_peers::bob(),
        })
        .expect("heartbeat");

    assert_eq!(
        presence_of_bob(&state, clock.now()),
        Some(Presence::Online),
        "evidence is the one and only exit from Unknown"
    );
}

#[test]
fn a_heartbeat_pulls_a_stale_peer_back_to_online() {
    let state = state_having_heard_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);
    clock.advance(DurationMillis::from_secs(40));

    let before = presence_of_bob(&state, clock.now());
    handler
        .handle(RecordPeerHeartbeat {
            peer: test_peers::bob(),
        })
        .expect("heartbeat");
    let after = presence_of_bob(&state, clock.now());

    assert_eq!(before, Some(Presence::Stale));
    assert_eq!(after, Some(Presence::Online));
}

#[test]
fn a_heartbeat_from_a_peer_with_no_address_is_rejected() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0));

    let outcome = handler_over(&state, &clock).handle(RecordPeerHeartbeat {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(PeerRosterError::UnknownPeer)),
        "evidence with nothing to dial is not something this context can act on"
    );
}

#[test]
fn the_local_peers_own_heartbeat_is_rejected() {
    let state = state_knowing_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));

    let outcome = handler_over(&state, &clock).handle(RecordPeerHeartbeat {
        peer: test_peers::alice(),
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(
            PeerRosterError::SelfConnection
        ))
    );
}

#[test]
fn a_heartbeat_leaves_the_session_alone() {
    let state = state_knowing_bob();
    state.modify(|roster| {
        roster
            .open_session(test_peers::bob(), SessionDirection::Inbound, T0)
            .expect("open");
    });
    let clock = Arc::new(ManualClock::starting_at(T0));
    clock.advance(DurationMillis::from_secs(3));

    handler_over(&state, &clock)
        .handle(RecordPeerHeartbeat {
            peer: test_peers::bob(),
        })
        .expect("heartbeat");

    assert_eq!(
        state.read(|roster| roster
            .peer(&test_peers::bob())
            .and_then(|entry| entry.session().map(crate::domain::Session::state))),
        Some(SessionState::Connecting),
        "presence and sessions are orthogonal: traffic on a link does not establish it"
    );
}
