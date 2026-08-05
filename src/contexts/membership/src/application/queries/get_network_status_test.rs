use std::num::NonZeroUsize;
use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::queries::{GetNetworkStatus, GetNetworkStatusHandler};
use crate::domain::{Endpoint, Millis, NetworkStatus, SessionDirection};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

#[test]
fn a_peer_with_no_session_reports_isolated() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let handler = GetNetworkStatusHandler::new(state);

    assert_eq!(handler.handle(GetNetworkStatus), NetworkStatus::Isolated);
}

#[test]
fn only_established_sessions_count_as_connected() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        for peer in [test_peers::bob(), test_peers::carol()] {
            roster
                .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
                .expect("discovery");
            roster
                .open_session(peer, SessionDirection::Outbound, T0)
                .expect("open");
        }
        roster
            .establish_session(test_peers::bob(), T0)
            .expect("only bob completes the handshake");
    });
    let handler = GetNetworkStatusHandler::new(state);

    assert_eq!(
        handler.handle(GetNetworkStatus),
        NetworkStatus::Connected(NonZeroUsize::new(1).expect("one peer")),
        "a session still connecting is not yet reachability"
    );
}

#[test]
fn a_join_in_flight_is_visible_as_joining() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let handler = GetNetworkStatusHandler::new(Arc::clone(&state));

    let phase = state.begin_join();
    assert_eq!(handler.handle(GetNetworkStatus), NetworkStatus::Joining);
    drop(phase);

    assert_eq!(handler.handle(GetNetworkStatus), NetworkStatus::Isolated);
}
