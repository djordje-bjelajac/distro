use shared_types::ProtocolVersion;

use crate::domain::{Endpoint, JoinTicket, Millis};
use crate::ports::port_fakes::{ScriptedDiscovery, UnavailableDiscovery};
use crate::ports::{DiscoveredPeer, PeerDiscoveryError, PeerDiscoveryPort};
use crate::test_peers;

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn bob() -> DiscoveredPeer {
    DiscoveredPeer {
        peer: test_peers::bob(),
        endpoints: vec![endpoint("/ip4/198.51.100.7/udp/4001/quic-v1")],
    }
}

fn ticket_from(discovered: &DiscoveredPeer) -> JoinTicket {
    JoinTicket::new(
        discovered.peer,
        discovered.endpoints.clone(),
        ProtocolVersion::CURRENT,
        Millis::from_millis(100_000),
    )
    .expect("well formed ticket")
}

#[test]
fn announcing_publishes_the_local_endpoints() {
    let discovery = ScriptedDiscovery::observing(vec![]);
    let port: &dyn PeerDiscoveryPort = &discovery;
    let endpoints = vec![endpoint("/ip4/198.51.100.1/udp/4001/quic-v1")];

    assert_eq!(port.announce(&endpoints), Ok(()));

    assert_eq!(discovery.announcements(), vec![endpoints]);
}

#[test]
fn observing_yields_the_peers_the_mechanism_has_seen() {
    let discovery = ScriptedDiscovery::observing(vec![bob()]);
    let port: &dyn PeerDiscoveryPort = &discovery;

    assert_eq!(port.observe_peers(), Ok(vec![bob()]));
}

#[test]
fn observing_an_empty_network_is_success_not_failure() {
    // Isolation is a normal state (canvas §2.2): a LAN with no neighbour and
    // an empty peer cache must not look like a discovery failure.
    let discovery = ScriptedDiscovery::observing(vec![]);
    let port: &dyn PeerDiscoveryPort = &discovery;

    assert_eq!(port.observe_peers(), Ok(Vec::new()));
}

#[test]
fn redeeming_a_ticket_dials_its_endpoints_and_yields_the_issuer() {
    let discovery = ScriptedDiscovery::observing(vec![]).with_redeemable(bob());
    let port: &dyn PeerDiscoveryPort = &discovery;

    assert_eq!(port.redeem_join_ticket(&ticket_from(&bob())), Ok(bob()));
}

#[test]
fn redeeming_a_ticket_nobody_answers_is_a_typed_error() {
    // AC3: a failed bootstrap must produce a visible diagnostic, never a hang.
    let discovery = ScriptedDiscovery::observing(vec![]);
    let port: &dyn PeerDiscoveryPort = &discovery;

    assert_eq!(
        port.redeem_join_ticket(&ticket_from(&bob())),
        Err(PeerDiscoveryError::TicketUnreachable)
    );
}

#[test]
fn the_port_does_not_re_check_ticket_validity() {
    // Validity is a pure domain rule (`JoinTicket::validate`) the application
    // applies before redeeming; duplicating it behind the port would put the
    // clock on both sides of the boundary.
    let discovery = ScriptedDiscovery::observing(vec![]).with_redeemable(bob());
    let port: &dyn PeerDiscoveryPort = &discovery;
    let expired = JoinTicket::new(
        test_peers::bob(),
        vec![endpoint("/ip4/198.51.100.7/udp/4001/quic-v1")],
        ProtocolVersion::CURRENT,
        Millis::ZERO,
    )
    .unwrap();

    assert_eq!(port.redeem_join_ticket(&expired), Ok(bob()));
}

#[test]
fn every_operation_reports_an_unavailable_mechanism_as_a_typed_error() {
    let discovery = UnavailableDiscovery;
    let port: &dyn PeerDiscoveryPort = &discovery;

    assert_eq!(
        port.announce(&[endpoint("/ip4/198.51.100.1/udp/4001")]),
        Err(PeerDiscoveryError::Unavailable)
    );
    assert_eq!(port.observe_peers(), Err(PeerDiscoveryError::Unavailable));
    assert_eq!(
        port.redeem_join_ticket(&ticket_from(&bob())),
        Err(PeerDiscoveryError::Unavailable)
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            PeerDiscoveryError::Unavailable,
            "peer discovery is not available",
        ),
        (
            PeerDiscoveryError::AnnouncementRejected,
            "the local peer's announcement was rejected",
        ),
        (
            PeerDiscoveryError::TicketUnreachable,
            "no endpoint in the join ticket answered",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
