use membership::domain::Endpoint;

use crate::composition::LocalEndpoints;

fn direct(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("a valid address")
}

fn relayed(address: &str) -> Endpoint {
    Endpoint::relayed(address).expect("a valid address")
}

#[test]
fn a_fresh_instance_knows_nowhere_it_can_be_reached() {
    // Which is why a ticket cannot be minted before the transport has said
    // anything: `JoinTicket::new` refuses one with no endpoints.
    assert!(LocalEndpoints::new().is_empty());
}

#[test]
fn a_listening_endpoint_is_remembered() {
    let endpoints = LocalEndpoints::new();

    assert!(endpoints.record_listening(direct("/ip4/192.168.0.2/udp/4001/quic-v1")));

    assert_eq!(
        endpoints.all(),
        vec![direct("/ip4/192.168.0.2/udp/4001/quic-v1")]
    );
}

#[test]
fn the_same_endpoint_is_not_remembered_twice() {
    // libp2p re-reports a listener on every interface change; a ticket with
    // the same address ten times is a worse ticket.
    let endpoints = LocalEndpoints::new();

    assert!(endpoints.record_listening(direct("/ip4/10.0.0.1/tcp/4001")));
    assert!(!endpoints.record_listening(direct("/ip4/10.0.0.1/tcp/4001")));

    assert_eq!(endpoints.all().len(), 1);
}

#[test]
fn externally_confirmed_endpoints_are_listed_first() {
    // A confirmed address is the one a stranger can dial; a listening address
    // may be a private range that means nothing outside this machine.
    let endpoints = LocalEndpoints::new();
    endpoints.record_listening(direct("/ip4/10.0.0.1/tcp/4001"));
    endpoints.record_confirmed(direct("/ip4/203.0.113.7/udp/4001/quic-v1"));

    assert_eq!(
        endpoints.all(),
        vec![
            direct("/ip4/203.0.113.7/udp/4001/quic-v1"),
            direct("/ip4/10.0.0.1/tcp/4001"),
        ]
    );
}

#[test]
fn a_listening_address_is_kept_even_once_a_public_one_is_confirmed() {
    // A LAN peer redeeming this ticket wants the private address.
    let endpoints = LocalEndpoints::new();
    endpoints.record_listening(direct("/ip4/10.0.0.1/tcp/4001"));
    endpoints.record_confirmed(relayed("/ip4/203.0.113.7/tcp/4001/p2p-circuit"));

    assert_eq!(endpoints.all().len(), 2);
    assert!(!endpoints.is_empty());
}

#[test]
fn an_address_confirmed_after_it_was_listened_on_is_not_duplicated() {
    let endpoints = LocalEndpoints::new();
    endpoints.record_listening(direct("/ip4/203.0.113.7/tcp/4001"));
    endpoints.record_confirmed(direct("/ip4/203.0.113.7/tcp/4001"));

    assert_eq!(endpoints.all(), vec![direct("/ip4/203.0.113.7/tcp/4001")]);
}
