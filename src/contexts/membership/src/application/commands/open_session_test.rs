use std::sync::Arc;

use shared_types::{PeerDisconnected, PeerId};

use crate::application::MembershipState;
use crate::application::commands::{
    EstablishSession, EstablishSessionHandler, OpenSession, OpenSessionHandler,
};
use crate::domain::events::MembershipEvent;
use crate::domain::{Endpoint, Millis, PeerRosterError, SessionDirection, SessionState};
use crate::ports::port_fakes::{ManualClock, RecordingPublisher, ScriptedTransport};
use crate::ports::{
    ClockPort, EventPublisherPort, MembershipCommandError, PeerTransportError, PeerTransportPort,
};
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
    open: OpenSessionHandler,
    establish: EstablishSessionHandler,
}

/// A fixture whose transport answers at `reachable`.
fn fixture_reaching(reachable: &[&str]) -> Fixture {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0)) as Arc<dyn ClockPort + Send + Sync>;
    let publisher = Arc::new(RecordingPublisher::new());
    let mut transport = ScriptedTransport::listening_on(vec![endpoint("/ip4/0.0.0.0/udp/4001")]);
    for address in reachable {
        transport = transport.reachable_at(endpoint(address));
    }
    let transport = Arc::new(transport);

    Fixture {
        open: OpenSessionHandler::new(
            Arc::clone(&state),
            Arc::clone(&clock),
            Arc::clone(&transport) as Arc<dyn PeerTransportPort + Send + Sync>,
            Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        ),
        establish: EstablishSessionHandler::new(
            Arc::clone(&state),
            clock,
            Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        ),
        state,
        transport,
        publisher,
    }
}

fn inbound(peer: PeerId, address: &str) -> OpenSession {
    OpenSession {
        peer,
        direction: SessionDirection::Inbound,
        endpoints: vec![endpoint(address)],
    }
}

fn outbound(peer: PeerId) -> OpenSession {
    OpenSession {
        peer,
        direction: SessionDirection::Outbound,
        endpoints: Vec::new(),
    }
}

fn session_state(f: &Fixture, peer: PeerId) -> Option<SessionState> {
    f.state.read(|roster| {
        roster
            .peer(&peer)
            .and_then(|entry| entry.session().map(|s| s.state()))
    })
}

#[test]
fn an_inbound_session_from_an_unknown_peer_enters_it_in_the_roster_first() {
    // The peer that redeemed *our* join ticket dials before we have ever
    // discovered it; refusing it would break the ladder from the other side.
    let f = fixture_reaching(&[]);

    let outcome = f
        .open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("an inbound link from a stranger is the normal way a ticket pays off");

    assert_eq!(outcome, crate::domain::SessionOutcome::quiet());
    assert_eq!(
        session_state(&f, test_peers::bob()),
        Some(SessionState::Connecting)
    );
    assert_eq!(
        f.transport.dialled(),
        Vec::new(),
        "the remote dialled us; we do not dial back"
    );
}

#[test]
fn opening_a_session_publishes_nothing_because_nothing_is_reachable_yet() {
    let f = fixture_reaching(&[]);

    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("open");

    assert_eq!(
        f.publisher.cross_context(),
        Vec::new(),
        "PeerConnected belongs to the handshake, not to the dial"
    );
}

#[test]
fn an_outbound_session_dials_the_addresses_the_roster_holds() {
    let f = fixture_reaching(&[BOB_ADDRESS]);
    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("discovery");
    f.state.modify(|roster| {
        roster
            .close_session(test_peers::bob())
            .expect("clear the inbound session so an outbound one is not a collapse");
    });

    f.open
        .handle(outbound(test_peers::bob()))
        .expect("the endpoint answers");

    assert_eq!(f.transport.dialled(), vec![test_peers::bob()]);
    assert_eq!(
        session_state(&f, test_peers::bob()),
        Some(SessionState::Connecting)
    );
}

#[test]
fn dialling_a_peer_the_roster_has_never_heard_of_is_rejected() {
    let f = fixture_reaching(&[BOB_ADDRESS]);

    let outcome = f.open.handle(outbound(test_peers::bob()));

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(PeerRosterError::UnknownPeer)),
        "a dial needs an address, and an address is what discovery is for"
    );
    assert_eq!(f.transport.dialled(), Vec::new());
}

#[test]
fn an_unreachable_peer_fails_with_the_transports_own_reason() {
    let f = fixture_reaching(&[]);
    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("discovery");
    f.state.modify(|roster| {
        roster.close_session(test_peers::bob()).expect("clear");
    });

    let outcome = f.open.handle(outbound(test_peers::bob()));

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Transport(
            PeerTransportError::NoReachableEndpoint
        )),
        "S7's known limit, stated rather than retried forever"
    );
    assert_eq!(
        session_state(&f, test_peers::bob()),
        None,
        "a dial that never answered leaves no session behind"
    );
}

#[test]
fn a_second_session_in_the_same_direction_is_rejected() {
    let f = fixture_reaching(&[]);
    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("first");

    let outcome = f.open.handle(inbound(test_peers::bob(), BOB_ADDRESS));

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(
            PeerRosterError::SessionAlreadyOpen
        ))
    );
}

#[test]
fn a_simultaneous_connect_reports_the_session_the_caller_must_close() {
    // bob's key sorts below alice's, so the session bob dialled survives
    // (invariant 3) and the outbound one this peer opened is superseded.
    let f = fixture_reaching(&[BOB_ADDRESS]);
    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("discovery");
    f.state.modify(|roster| {
        roster.close_session(test_peers::bob()).expect("clear");
    });
    f.open.handle(outbound(test_peers::bob())).expect("we dial");

    let outcome = f
        .open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("they dial at the same time — the normal case, not an edge case");

    assert_eq!(outcome.superseded, Some(SessionDirection::Outbound));
    assert_eq!(
        outcome
            .collapse
            .expect("a collapse was decided")
            .initiator(),
        test_peers::bob()
    );
    assert_eq!(outcome.disconnected, None, "the loser had not established");
    assert_eq!(
        f.transport.closed(),
        Vec::new(),
        "the transport closes by peer and cannot name one of two links; the caller that \
         accepted them can, so the superseded direction is reported instead"
    );
}

#[test]
fn a_collapse_that_discards_an_established_link_reports_the_disconnect() {
    // An honest gap: the peer really does stop being reachable until the
    // survivor handshakes, and messaging must not believe otherwise.
    let f = fixture_reaching(&[BOB_ADDRESS]);
    f.open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("discovery");
    f.state.modify(|roster| {
        roster.close_session(test_peers::bob()).expect("clear");
    });
    f.open.handle(outbound(test_peers::bob())).expect("we dial");
    f.establish
        .handle(EstablishSession {
            peer: test_peers::bob(),
        })
        .expect("our dial completes first");

    let outcome = f
        .open
        .handle(inbound(test_peers::bob(), BOB_ADDRESS))
        .expect("their dial arrives after ours established");

    assert_eq!(
        outcome.disconnected,
        Some(PeerDisconnected {
            peer: test_peers::bob()
        })
    );
    assert!(
        f.publisher
            .cross_context()
            .contains(&MembershipEvent::PeerDisconnected(PeerDisconnected {
                peer: test_peers::bob()
            })),
        "the disconnect reaches other contexts, not just the caller"
    );
}
