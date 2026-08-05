use std::num::NonZeroUsize;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::domain::{Endpoint, Millis, NetworkStatus, SessionDirection};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn connected(count: usize) -> NetworkStatus {
    NetworkStatus::Connected(NonZeroUsize::new(count).expect("test expects a real count"))
}

/// A state local to `alice` holding an established session to `peer`.
fn state_connected_to(peer: PeerId) -> MembershipState {
    let state = MembershipState::for_local_peer(test_peers::alice());
    state.modify(|roster| {
        roster
            .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
            .expect("discovery of another peer is legal");
        roster
            .open_session(peer, SessionDirection::Outbound, T0)
            .expect("first session for the peer");
        roster
            .establish_session(peer, T0)
            .expect("handshake completes");
    });
    state
}

#[test]
fn a_fresh_state_is_isolated_rather_than_failed() {
    let state = MembershipState::for_local_peer(test_peers::alice());

    assert_eq!(state.local_peer(), test_peers::alice());
    assert_eq!(
        state.network_status(),
        NetworkStatus::Isolated,
        "a peer that has not joined anything yet is isolated, which is a normal state"
    );
}

#[test]
fn an_established_session_makes_the_status_connected() {
    let state = state_connected_to(test_peers::bob());

    assert_eq!(state.network_status(), connected(1));
}

#[test]
fn the_status_reports_joining_while_the_bootstrap_ladder_runs() {
    let state = MembershipState::for_local_peer(test_peers::alice());

    let phase = state.begin_join();
    assert_eq!(
        state.network_status(),
        NetworkStatus::Joining,
        "the ladder is in flight, which the count alone could never say"
    );
    drop(phase);

    assert_eq!(state.network_status(), NetworkStatus::Isolated);
}

#[test]
fn a_join_in_flight_outranks_the_session_count() {
    let state = state_connected_to(test_peers::bob());

    let phase = state.begin_join();
    assert_eq!(
        state.network_status(),
        NetworkStatus::Joining,
        "a join that is running is what the user is waiting on, connected or not"
    );
    drop(phase);

    assert_eq!(state.network_status(), connected(1));
}

#[test]
fn the_joining_phase_ends_even_when_the_ladder_returns_early() {
    let state = MembershipState::for_local_peer(test_peers::alice());

    // Stands in for a handler that gives up mid-ladder with `?`.
    fn abandoned_join(state: &MembershipState) -> Result<(), &'static str> {
        let _phase = state.begin_join();
        Err("the event publisher was unavailable")
    }

    assert!(abandoned_join(&state).is_err());
    assert_eq!(
        state.network_status(),
        NetworkStatus::Isolated,
        "a status latched on Joining would be exactly the hang AC3 forbids"
    );
}

#[test]
fn reading_the_roster_never_holds_the_lock_across_the_callers_own_work() {
    // The bootstrap ladder calls ports while a join phase is live, and those
    // ports may ask for the status again (a UI redraw, a diagnostic). Both
    // accessors must therefore be re-entrant with respect to the status read.
    let state = state_connected_to(test_peers::bob());

    let observed = state.read(|roster| {
        let _ = roster.len();
        NetworkStatus::from_connected_peers(roster.established_session_count())
    });

    assert_eq!(observed, connected(1));
    assert_eq!(state.network_status(), connected(1));
}
