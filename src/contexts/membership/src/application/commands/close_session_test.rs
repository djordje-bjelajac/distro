use std::sync::Arc;

use shared_types::{PeerDisconnected, PeerId};

use crate::application::MembershipState;
use crate::application::commands::{CloseSession, CloseSessionHandler, SessionCloseCause};
use crate::domain::events::MembershipEvent;
use crate::domain::{Endpoint, Millis, PeerRosterError, SessionDirection};
use crate::ports::port_fakes::{ManualClock, RecordingPublisher, ScriptedTransport};
use crate::ports::{ClockPort, EventPublisherPort, MembershipCommandError, PeerTransportPort};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);
const BOB_ADDRESS: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

struct Fixture {
    state: Arc<MembershipState>,
    transport: Arc<ScriptedTransport>,
    publisher: Arc<RecordingPublisher>,
    handler: CloseSessionHandler,
}

/// A fixture whose roster holds a session to `bob`, established or not, and
/// whose transport has a live link to him.
fn fixture_with_session(established: bool) -> Fixture {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let transport =
        Arc::new(ScriptedTransport::listening_on(Vec::new()).reachable_at(endpoint(BOB_ADDRESS)));
    let publisher = Arc::new(RecordingPublisher::new());

    state.modify(|roster| {
        roster
            .record_discovery(test_peers::bob(), vec![endpoint(BOB_ADDRESS)], T0)
            .expect("discovery");
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
        if established {
            roster
                .establish_session(test_peers::bob(), T0)
                .expect("establish");
        }
    });
    // Give the transport a link to close, as a real dial would have.
    transport
        .dial(test_peers::bob(), &[endpoint(BOB_ADDRESS)])
        .expect("scripted dial");

    let handler = CloseSessionHandler::new(
        Arc::clone(&state),
        Arc::clone(&transport) as Arc<dyn PeerTransportPort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    Fixture {
        state,
        transport,
        publisher,
        handler,
    }
}

fn disconnected(peer: PeerId) -> MembershipEvent {
    MembershipEvent::PeerDisconnected(PeerDisconnected { peer })
}

#[test]
fn closing_an_established_session_announces_the_disconnect() {
    let f = fixture_with_session(true);

    let outcome = f
        .handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("close");

    assert_eq!(
        outcome.disconnected,
        Some(PeerDisconnected {
            peer: test_peers::bob()
        })
    );
    assert_eq!(
        f.publisher.cross_context(),
        vec![disconnected(test_peers::bob())]
    );
    assert_eq!(f.state.read(|roster| roster.established_session_count()), 0);
}

#[test]
fn closing_a_session_that_never_established_announces_nothing() {
    // No PeerConnected was ever published for it, and an unmatched disconnect
    // would make messaging fail directs for a peer it never thought reachable.
    let f = fixture_with_session(false);

    let outcome = f
        .handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("close");

    assert_eq!(outcome.disconnected, None);
    assert_eq!(f.publisher.cross_context(), Vec::new());
}

#[test]
fn a_local_decision_also_closes_the_link_at_the_transport() {
    let f = fixture_with_session(true);

    f.handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("close");

    assert_eq!(f.transport.closed(), vec![test_peers::bob()]);
}

#[test]
fn a_link_the_transport_reported_dead_is_not_closed_again() {
    let f = fixture_with_session(true);

    f.handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::TransportReported,
        })
        .expect("close");

    assert_eq!(
        f.transport.closed(),
        Vec::new(),
        "the transport is the one reporting; asking it to close what it just lost is noise"
    );
    assert_eq!(
        f.publisher.cross_context(),
        vec![disconnected(test_peers::bob())],
        "the roster still ends the session and still announces it"
    );
}

#[test]
fn closing_a_peer_with_no_session_is_rejected() {
    let f = fixture_with_session(true);
    f.handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("first close");

    let second = f.handler.handle(CloseSession {
        peer: test_peers::bob(),
        cause: SessionCloseCause::LocalDecision,
    });

    assert_eq!(
        second,
        Err(MembershipCommandError::Roster(PeerRosterError::NoSession)),
        "PeerDisconnected must not be published twice for one link"
    );
    assert_eq!(f.publisher.cross_context().len(), 1);
}

#[test]
fn the_peer_stays_known_after_its_session_ends() {
    let f = fixture_with_session(true);

    f.handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("close");

    assert!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).is_some()),
        "a closed link is not a forgotten peer; its addresses are next launch's warm start"
    );
}

#[test]
fn a_transport_that_cannot_close_does_not_stop_the_session_from_ending() {
    // The link is being abandoned either way, and a transport that reports
    // NoSuchSession is telling us the remote already closed it.
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        roster
            .record_discovery(test_peers::bob(), vec![endpoint(BOB_ADDRESS)], T0)
            .expect("discovery");
        roster
            .open_session(test_peers::bob(), SessionDirection::Inbound, T0)
            .expect("open");
        roster
            .establish_session(test_peers::bob(), T0)
            .expect("establish");
    });
    let publisher = Arc::new(RecordingPublisher::new());
    // A transport that never dialled bob holds no link to close.
    let transport = Arc::new(ScriptedTransport::listening_on(Vec::new()));
    let handler = CloseSessionHandler::new(
        Arc::clone(&state),
        Arc::clone(&transport) as Arc<dyn PeerTransportPort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    let outcome = handler.handle(CloseSession {
        peer: test_peers::bob(),
        cause: SessionCloseCause::LocalDecision,
    });

    assert!(outcome.is_ok());
    assert_eq!(state.read(|roster| roster.established_session_count()), 0);
    assert_eq!(
        publisher.cross_context(),
        vec![disconnected(test_peers::bob())]
    );
}

#[test]
fn the_clock_is_not_consulted_because_a_close_is_not_evidence_of_life() {
    let f = fixture_with_session(true);
    let clock = ManualClock::starting_at(T0);
    let before = clock.now();

    f.handler
        .handle(CloseSession {
            peer: test_peers::bob(),
            cause: SessionCloseCause::LocalDecision,
        })
        .expect("close");

    assert_eq!(clock.now(), before);
    assert_eq!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).map(|e| e.last_seen_at())),
        Some(T0),
        "a locally initiated close says nothing about the remote at all"
    );
}
