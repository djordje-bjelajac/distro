use std::sync::Arc;

use shared_types::PeerConnected;

use crate::application::MembershipState;
use crate::application::commands::{EstablishSession, EstablishSessionHandler};
use crate::domain::events::MembershipEvent;
use crate::domain::{
    DurationMillis, Endpoint, Millis, PeerRosterError, SessionDirection, SessionState,
};
use crate::ports::port_fakes::{FailingPublisher, ManualClock, RecordingPublisher};
use crate::ports::{ClockPort, EventPublisherError, EventPublisherPort, MembershipCommandError};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

/// `alice`'s state with a connecting inbound session to `bob`.
fn state_connecting_to_bob() -> Arc<MembershipState> {
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
            .open_session(test_peers::bob(), SessionDirection::Inbound, T0)
            .expect("open");
    });
    state
}

fn handler_over(
    state: &Arc<MembershipState>,
    clock: &Arc<ManualClock>,
    publisher: &Arc<RecordingPublisher>,
) -> EstablishSessionHandler {
    EstablishSessionHandler::new(
        Arc::clone(state),
        Arc::clone(clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::clone(publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    )
}

#[test]
fn a_completed_handshake_is_the_one_place_peer_connected_is_published() {
    let state = state_connecting_to_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());

    let outcome = handler_over(&state, &clock, &publisher)
        .handle(EstablishSession {
            peer: test_peers::bob(),
        })
        .expect("the handshake completed");

    assert_eq!(
        outcome.connected,
        Some(PeerConnected {
            peer: test_peers::bob()
        })
    );
    assert_eq!(
        publisher.cross_context(),
        vec![MembershipEvent::PeerConnected(PeerConnected {
            peer: test_peers::bob()
        })]
    );
    assert_eq!(
        state.read(|roster| roster.established_session_count()),
        1,
        "only now is the peer reachable"
    );
}

#[test]
fn a_completed_handshake_is_evidence_of_life() {
    let state = state_connecting_to_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    clock.advance(DurationMillis::from_secs(7));

    let last_seen_at =
        || state.read(|roster| roster.peer(&test_peers::bob()).map(|e| e.last_seen_at()));
    assert_eq!(
        last_seen_at(),
        Some(Some(T0)),
        "the inbound open at T0 was itself evidence — a remote that dialled us just acted"
    );

    handler_over(&state, &clock, &publisher)
        .handle(EstablishSession {
            peer: test_peers::bob(),
        })
        .expect("establish");

    assert_eq!(
        last_seen_at(),
        Some(Some(T0.saturating_add(DurationMillis::from_secs(7)))),
        "and the completed handshake is later evidence still: the remote used its \
         secret key in a live exchange, which moves the instant forward"
    );
}

#[test]
fn establishing_a_peer_with_no_session_is_rejected() {
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
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());

    let outcome = handler_over(&state, &clock, &publisher).handle(EstablishSession {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(PeerRosterError::NoSession))
    );
    assert_eq!(publisher.published(), Vec::new());
}

#[test]
fn establishing_twice_is_rejected_so_peer_connected_is_published_once_per_link() {
    let state = state_connecting_to_bob();
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    let handler = handler_over(&state, &clock, &publisher);

    handler
        .handle(EstablishSession {
            peer: test_peers::bob(),
        })
        .expect("first handshake");
    let second = handler.handle(EstablishSession {
        peer: test_peers::bob(),
    });

    assert_eq!(
        second,
        Err(MembershipCommandError::Roster(
            PeerRosterError::InvalidSessionTransition {
                from: SessionState::Established,
                to: SessionState::Established,
            }
        ))
    );
    assert_eq!(publisher.cross_context().len(), 1);
}

#[test]
fn a_publisher_failure_leaves_the_session_established_and_says_so() {
    let state = state_connecting_to_bob();
    let handler = EstablishSessionHandler::new(
        Arc::clone(&state),
        Arc::new(ManualClock::starting_at(T0)) as Arc<dyn ClockPort + Send + Sync>,
        Arc::new(FailingPublisher(EventPublisherError::Unavailable))
            as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    let outcome = handler.handle(EstablishSession {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Publisher(
            EventPublisherError::Unavailable
        )),
        "the link is up and its consumers were not told; that is its own failure"
    );
    assert_eq!(state.read(|roster| roster.established_session_count()), 1);
}
