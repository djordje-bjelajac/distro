use libp2p::Multiaddr;
use membership::domain::{Endpoint, Reachability};

use crate::mapping::{EndpointMapping, EndpointMappingError, PeerIdMapping};
use crate::test_peers::{alice, bob};

fn relay_circuit() -> String {
    let relay = PeerIdMapping::to_libp2p(alice()).expect("maps out");
    let target = PeerIdMapping::to_libp2p(bob()).expect("maps out");

    format!("/ip4/198.51.100.7/udp/4001/quic-v1/p2p/{relay}/p2p-circuit/p2p/{target}")
}

#[test]
fn classifies_a_plain_quic_address_as_direct() {
    let endpoint = EndpointMapping::parse("/ip4/127.0.0.1/udp/4001/quic-v1").expect("parses");

    assert_eq!(endpoint.reachability(), Reachability::Direct);
    assert!(!endpoint.is_relayed());
}

#[test]
fn classifies_a_plain_tcp_address_as_direct() {
    let endpoint = EndpointMapping::parse("/ip4/10.0.0.4/tcp/4001").expect("parses");

    assert_eq!(endpoint.reachability(), Reachability::Direct);
}

#[test]
fn classifies_a_circuit_address_as_relayed() {
    // AC12 rests on exactly this: a relayed endpoint is one a *third peer* is
    // carrying, which the roster, the UI, and S7's diagnostic all read off the
    // reachability class.
    let endpoint = EndpointMapping::parse(&relay_circuit()).expect("parses");

    assert_eq!(endpoint.reachability(), Reachability::Relayed);
    assert!(endpoint.is_relayed());
}

#[test]
fn classifies_a_circuit_address_without_a_target_suffix_as_relayed() {
    let relay = PeerIdMapping::to_libp2p(alice()).expect("maps out");
    let address = format!("/ip4/198.51.100.7/tcp/4001/p2p/{relay}/p2p-circuit");

    assert_eq!(
        EndpointMapping::parse(&address)
            .expect("parses")
            .reachability(),
        Reachability::Relayed
    );
}

#[test]
fn round_trips_a_direct_address_unchanged() {
    let original: Multiaddr = "/ip4/192.0.2.1/udp/4001/quic-v1".parse().expect("parses");

    let endpoint = EndpointMapping::to_endpoint(&original).expect("maps in");
    let returned = EndpointMapping::to_multiaddr(&endpoint).expect("maps out");

    assert_eq!(returned, original);
    assert_eq!(endpoint.reachability(), Reachability::Direct);
}

#[test]
fn round_trips_a_relayed_address_unchanged() {
    let original: Multiaddr = relay_circuit().parse().expect("parses");

    let endpoint = EndpointMapping::to_endpoint(&original).expect("maps in");
    let returned = EndpointMapping::to_multiaddr(&endpoint).expect("maps out");

    assert_eq!(returned, original);
    assert_eq!(endpoint.reachability(), Reachability::Relayed);
}

#[test]
fn round_trips_every_transport_shape_this_build_speaks() {
    let addresses = [
        "/ip4/127.0.0.1/tcp/0",
        "/ip4/0.0.0.0/udp/0/quic-v1",
        "/ip6/::1/udp/4001/quic-v1",
        "/ip6/::/tcp/4001",
        "/ip4/203.0.113.9/udp/40001/quic-v1",
    ];

    for address in addresses {
        let parsed: Multiaddr = address.parse().expect("fixture parses");
        let endpoint = EndpointMapping::to_endpoint(&parsed).expect("maps in");

        assert_eq!(
            EndpointMapping::to_multiaddr(&endpoint).expect("maps out"),
            parsed,
            "{address} did not survive the round trip"
        );
        assert_eq!(endpoint.reachability(), Reachability::Direct);
    }
}

#[test]
fn refuses_text_that_is_not_a_multiaddress() {
    assert_eq!(
        EndpointMapping::parse("192.168.1.1:4001"),
        Err(EndpointMappingError::MalformedAddress)
    );
    assert_eq!(
        EndpointMapping::parse("not an address at all"),
        Err(EndpointMappingError::MalformedAddress)
    );
    assert_eq!(
        EndpointMapping::parse(""),
        Err(EndpointMappingError::MalformedAddress)
    );
}

#[test]
fn refuses_a_multiaddress_with_an_unknown_protocol() {
    assert_eq!(
        EndpointMapping::parse("/ip4/127.0.0.1/nonsense/4001"),
        Err(EndpointMappingError::MalformedAddress)
    );
}

#[test]
fn an_endpoint_whose_text_is_not_an_address_cannot_be_dialled() {
    // `Endpoint` accepts any bounded, control-character-free string by design;
    // the structural check is here, and it refuses rather than dialling
    // nonsense.
    let endpoint = Endpoint::direct("hello, world").expect("the domain accepts this");

    assert_eq!(
        EndpointMapping::to_multiaddr(&endpoint),
        Err(EndpointMappingError::MalformedAddress)
    );
}

#[test]
fn refuses_an_address_longer_than_the_domain_admits() {
    let long_path = "a".repeat(Endpoint::MAX_ADDRESS_BYTES);
    let address = format!("/ip4/127.0.0.1/tcp/4001/p2p-circuit/memory/{long_path}");

    assert!(matches!(
        EndpointMapping::parse(&address),
        Err(EndpointMappingError::MalformedAddress | EndpointMappingError::Rejected(_))
    ));
}

#[test]
fn trims_surrounding_whitespace_the_way_a_pasted_address_arrives() {
    let endpoint =
        EndpointMapping::parse("  /ip4/127.0.0.1/udp/4001/quic-v1\t").expect("parses trimmed");

    assert_eq!(endpoint.address(), "/ip4/127.0.0.1/udp/4001/quic-v1");
}
