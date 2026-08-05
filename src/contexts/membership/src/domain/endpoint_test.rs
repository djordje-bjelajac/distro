use crate::domain::{Endpoint, EndpointError, Reachability};

const DIRECT_ADDRESS: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";
const RELAYED_ADDRESS: &str = "/ip4/203.0.113.9/udp/4001/quic-v1/p2p-circuit";

#[test]
fn keeps_the_address_verbatim_and_its_reachability_class() {
    let endpoint = Endpoint::direct(DIRECT_ADDRESS).expect("a plain multiaddress is accepted");

    assert_eq!(endpoint.address(), DIRECT_ADDRESS);
    assert_eq!(endpoint.reachability(), Reachability::Direct);
    assert!(!endpoint.is_relayed());
}

#[test]
fn a_relayed_endpoint_is_marked_as_such() {
    let endpoint = Endpoint::relayed(RELAYED_ADDRESS).expect("a circuit address is accepted");

    assert_eq!(endpoint.reachability(), Reachability::Relayed);
    assert!(endpoint.is_relayed());
}

#[test]
fn the_address_is_opaque_to_the_domain() {
    // Full multiaddress parsing is an adapter concern (canvas §2.2): anything
    // printable and bounded is a legal endpoint here, so a future transport
    // syntax needs no domain change.
    let endpoint = Endpoint::direct("not-a-multiaddress-but-still-opaque")
        .expect("the domain does not parse addresses");

    assert_eq!(endpoint.address(), "not-a-multiaddress-but-still-opaque");
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    let endpoint =
        Endpoint::direct("  /ip4/198.51.100.7/udp/4001/quic-v1\n").expect("padding is trimmed");

    assert_eq!(endpoint.address(), DIRECT_ADDRESS);
}

#[test]
fn rejects_an_address_that_is_empty_after_trimming() {
    assert_eq!(Endpoint::direct(""), Err(EndpointError::Empty));
    assert_eq!(Endpoint::direct("   \t "), Err(EndpointError::Empty));
}

#[test]
fn accepts_an_address_exactly_at_the_length_cap() {
    let address = "a".repeat(Endpoint::MAX_ADDRESS_BYTES);

    let endpoint = Endpoint::direct(&address).expect("the cap itself is allowed");

    assert_eq!(endpoint.address().len(), Endpoint::MAX_ADDRESS_BYTES);
}

#[test]
fn rejects_an_address_one_byte_over_the_length_cap() {
    let address = "a".repeat(Endpoint::MAX_ADDRESS_BYTES + 1);

    assert_eq!(
        Endpoint::direct(&address),
        Err(EndpointError::TooLong {
            bytes: Endpoint::MAX_ADDRESS_BYTES + 1,
            limit: Endpoint::MAX_ADDRESS_BYTES,
        })
    );
}

#[test]
fn rejects_a_control_character_inside_the_address() {
    assert_eq!(
        Endpoint::direct("/ip4/198.51.100.7\u{7}/udp/4001"),
        Err(EndpointError::ContainsControlCharacter)
    );
}

#[test]
fn equality_covers_both_the_address_and_the_reachability_class() {
    let direct = Endpoint::direct(DIRECT_ADDRESS).unwrap();
    let relayed = Endpoint::relayed(DIRECT_ADDRESS).unwrap();

    assert_eq!(direct, Endpoint::direct(DIRECT_ADDRESS).unwrap());
    assert_ne!(
        direct, relayed,
        "the same address reached through a relay is a different endpoint"
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            EndpointError::Empty,
            "endpoint address is empty after trimming",
        ),
        (
            EndpointError::ContainsControlCharacter,
            "endpoint address contains a control character",
        ),
        (
            EndpointError::TooLong {
                bytes: 300,
                limit: 256,
            },
            "endpoint address is 300 bytes, limit is 256",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
