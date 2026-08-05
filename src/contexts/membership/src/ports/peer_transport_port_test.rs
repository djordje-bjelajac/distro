use crate::domain::{Endpoint, Reachability};
use crate::ports::port_fakes::{ScriptedTransport, UnusableTransport};
use crate::ports::{PeerTransportError, PeerTransportPort};
use crate::test_peers;

const DIRECT: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";
const RELAYED: &str = "/ip4/203.0.113.9/udp/4001/quic-v1/p2p-circuit";

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

#[test]
fn listening_reports_the_endpoints_others_can_dial() {
    let transport = ScriptedTransport::listening_on(vec![endpoint(DIRECT)]);
    let port: &dyn PeerTransportPort = &transport;

    assert_eq!(port.listen(), Ok(vec![endpoint(DIRECT)]));
}

#[test]
fn dialling_answers_with_the_endpoint_that_worked() {
    // Which endpoint answered decides whether the link is direct or relayed,
    // which is what the UI needs to explain a connection (S7, AC12).
    let relayed = Endpoint::relayed(RELAYED).unwrap();
    let transport = ScriptedTransport::listening_on(vec![]).reachable_at(relayed.clone());
    let port: &dyn PeerTransportPort = &transport;

    let answered = port
        .dial(test_peers::bob(), &[endpoint(DIRECT), relayed])
        .expect("the direct endpoint is unreachable, the relayed one answers");

    assert_eq!(answered.address(), RELAYED);
    assert_eq!(answered.reachability(), Reachability::Relayed);
}

#[test]
fn dialling_tries_endpoints_in_the_order_given() {
    let transport = ScriptedTransport::listening_on(vec![])
        .reachable_at(endpoint(DIRECT))
        .reachable_at(endpoint(RELAYED));
    let port: &dyn PeerTransportPort = &transport;

    let answered = port
        .dial(test_peers::bob(), &[endpoint(DIRECT), endpoint(RELAYED)])
        .unwrap();

    assert_eq!(answered.address(), DIRECT);
    assert_eq!(answered.reachability(), Reachability::Direct);
}

#[test]
fn a_peer_no_endpoint_reaches_is_a_typed_error_not_a_hang() {
    // S7: two symmetric-NAT peers with no relay available is a real outcome
    // the UI must be able to state.
    let transport = ScriptedTransport::listening_on(vec![]);
    let port: &dyn PeerTransportPort = &transport;

    assert_eq!(
        port.dial(test_peers::bob(), &[endpoint(DIRECT)]),
        Err(PeerTransportError::NoReachableEndpoint)
    );
}

#[test]
fn dialling_with_no_endpoints_at_all_reports_the_same_unreachability() {
    let transport = ScriptedTransport::listening_on(vec![]);
    let port: &dyn PeerTransportPort = &transport;

    assert_eq!(
        port.dial(test_peers::bob(), &[]),
        Err(PeerTransportError::NoReachableEndpoint)
    );
}

#[test]
fn closing_a_session_reaches_the_transport() {
    let transport = ScriptedTransport::listening_on(vec![]).reachable_at(endpoint(DIRECT));
    let port: &dyn PeerTransportPort = &transport;
    port.dial(test_peers::bob(), &[endpoint(DIRECT)]).unwrap();

    assert_eq!(port.close_session(test_peers::bob()), Ok(()));

    assert_eq!(transport.closed(), vec![test_peers::bob()]);
}

#[test]
fn closing_a_session_the_transport_does_not_hold_is_a_typed_error() {
    let transport = ScriptedTransport::listening_on(vec![]);
    let port: &dyn PeerTransportPort = &transport;

    assert_eq!(
        port.close_session(test_peers::bob()),
        Err(PeerTransportError::NoSuchSession)
    );
}

#[test]
fn an_unusable_transport_fails_every_operation_with_a_typed_error() {
    let transport = UnusableTransport(PeerTransportError::Unavailable);
    let port: &dyn PeerTransportPort = &transport;

    assert_eq!(port.listen(), Err(PeerTransportError::Unavailable));
    assert_eq!(
        port.dial(test_peers::bob(), &[endpoint(DIRECT)]),
        Err(PeerTransportError::Unavailable)
    );
    assert_eq!(
        port.close_session(test_peers::bob()),
        Err(PeerTransportError::Unavailable)
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            PeerTransportError::Unavailable,
            "the peer transport is not available",
        ),
        (
            PeerTransportError::ListenFailed,
            "the peer transport could not start listening",
        ),
        (
            PeerTransportError::NoReachableEndpoint,
            "no endpoint of the peer could be reached",
        ),
        (
            PeerTransportError::HandshakeFailed,
            "the session handshake with the peer failed",
        ),
        (
            PeerTransportError::NoSuchSession,
            "the transport holds no session for the peer",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
