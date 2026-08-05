use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::queries::{ListOnlinePeers, ListOnlinePeersHandler};
use crate::domain::{DurationMillis, Endpoint, LivenessWindows, Millis, SessionDirection};
use crate::ports::ClockPort;
use crate::ports::port_fakes::ManualClock;
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn state_knowing(peers: &[(shared_types::PeerId, Millis)]) -> Arc<MembershipState> {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        for (peer, seen_at) in peers {
            roster
                .record_discovery(
                    *peer,
                    vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                    *seen_at,
                )
                .expect("discovery");
        }
    });
    state
}

fn handler_over(state: &Arc<MembershipState>, clock: &Arc<ManualClock>) -> ListOnlinePeersHandler {
    ListOnlinePeersHandler::new(
        Arc::clone(state),
        Arc::clone(clock) as Arc<dyn ClockPort + Send + Sync>,
        LivenessWindows::DEFAULT,
    )
}

#[test]
fn a_peer_with_fresh_evidence_is_online() {
    let state = state_knowing(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    assert_eq!(
        handler_over(&state, &clock).handle(ListOnlinePeers),
        vec![test_peers::bob()]
    );
}

#[test]
fn a_peer_whose_evidence_aged_out_drops_off_the_list_without_any_write() {
    let state = state_knowing(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    clock.advance(DurationMillis::from_secs(31));

    assert_eq!(
        handler.handle(ListOnlinePeers),
        Vec::new(),
        "presence is derived from evidence age, not asserted by anyone (invariant 7)"
    );
}

#[test]
fn online_is_about_evidence_of_life_not_about_holding_a_session() {
    let state = state_knowing(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    // A peer that was seen but never dialled is online and not connected;
    // the two questions have different answers and different queries.
    state.modify(|roster| {
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
    });

    assert_eq!(
        handler_over(&state, &clock).handle(ListOnlinePeers),
        vec![test_peers::bob()]
    );
    assert_eq!(state.read(|roster| roster.established_session_count()), 0);
}

#[test]
fn the_list_is_ordered_by_peer_id() {
    let state = state_knowing(&[
        (test_peers::erin(), T0),
        (test_peers::bob(), T0),
        (test_peers::carol(), T0),
    ]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    let online = handler_over(&state, &clock).handle(ListOnlinePeers);

    let mut expected = vec![test_peers::erin(), test_peers::bob(), test_peers::carol()];
    expected.sort_unstable();
    assert_eq!(online, expected);
}
