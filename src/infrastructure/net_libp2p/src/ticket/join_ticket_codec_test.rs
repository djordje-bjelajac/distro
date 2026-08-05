use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ciborium::value::Value;
use membership::domain::{DurationMillis, Endpoint, JoinTicket, Millis, Reachability};
use shared_types::ProtocolVersion;

use crate::limits::ResourceLimits;
use crate::mapping::PeerIdMapping;
use crate::test_peers::{alice, bob};
use crate::ticket::{JoinTicketCodec, JoinTicketCodecError};

fn direct_endpoint() -> Endpoint {
    Endpoint::direct("/ip4/198.51.100.7/udp/4001/quic-v1").expect("valid endpoint")
}

fn relayed_endpoint() -> Endpoint {
    let relay = PeerIdMapping::to_libp2p(bob()).expect("maps out");
    let target = PeerIdMapping::to_libp2p(alice()).expect("maps out");

    Endpoint::relayed(&format!(
        "/ip4/203.0.113.2/udp/4001/quic-v1/p2p/{relay}/p2p-circuit/p2p/{target}"
    ))
    .expect("valid endpoint")
}

fn ticket(endpoints: Vec<Endpoint>) -> JoinTicket {
    JoinTicket::new(
        alice(),
        endpoints,
        ProtocolVersion::CURRENT,
        Millis::from_millis(1_700_000_000_000),
    )
    .expect("valid ticket")
}

fn body(fields: Vec<(&str, Value)>) -> String {
    let map = Value::Map(
        fields
            .into_iter()
            .map(|(name, value)| (Value::Text(name.to_owned()), value))
            .collect(),
    );
    let mut bytes = Vec::new();
    ciborium::into_writer(&map, &mut bytes).expect("fixture encodes");

    format!(
        "{}{}",
        JoinTicketCodec::PREFIX,
        URL_SAFE_NO_PAD.encode(&bytes)
    )
}

fn well_formed_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("issuer", Value::Bytes(alice().as_bytes().to_vec())),
        (
            "endpoints",
            Value::Array(vec![Value::Text(direct_endpoint().address().to_owned())]),
        ),
        ("protocol_major", Value::Integer(1.into())),
        ("protocol_minor", Value::Integer(0.into())),
        ("expires_at_millis", Value::Integer(42_u64.into())),
    ]
}

// ---------------------------------------------------------------- round trip

#[test]
fn round_trips_a_single_endpoint_ticket() {
    let original = ticket(vec![direct_endpoint()]);

    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&original)).expect("decodes");

    assert_eq!(decoded, original);
}

#[test]
fn round_trips_a_ticket_with_direct_and_relayed_endpoints() {
    let original = ticket(vec![direct_endpoint(), relayed_endpoint()]);

    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&original)).expect("decodes");

    assert_eq!(decoded, original);
    assert_eq!(decoded.endpoints()[0].reachability(), Reachability::Direct);
    assert_eq!(decoded.endpoints()[1].reachability(), Reachability::Relayed);
}

#[test]
fn preserves_the_issuer_protocol_and_expiry_exactly() {
    let original = JoinTicket::expiring_after(
        bob(),
        vec![direct_endpoint()],
        ProtocolVersion::new(3, 11),
        Millis::from_millis(1_000),
        DurationMillis::from_secs(3_600),
    )
    .expect("valid ticket");

    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&original)).expect("decodes");

    assert_eq!(decoded.issuer(), bob());
    assert_eq!(decoded.protocol(), ProtocolVersion::new(3, 11));
    assert_eq!(decoded.expires_at(), Millis::from_millis(3_601_000));
}

#[test]
fn survives_the_far_end_of_the_timeline() {
    let original = ticket(vec![direct_endpoint()]);
    let forever = JoinTicket::new(
        alice(),
        original.endpoints().to_vec(),
        ProtocolVersion::CURRENT,
        Millis::MAX,
    )
    .expect("valid ticket");

    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&forever)).expect("decodes");

    assert_eq!(decoded.expires_at(), Millis::MAX);
}

#[test]
fn the_string_is_self_describing_and_paste_safe() {
    let encoded = JoinTicketCodec::encode(&ticket(vec![direct_endpoint(), relayed_endpoint()]));

    assert!(encoded.starts_with("distro-join-1."));
    assert!(
        encoded
            .chars()
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.')),
        "a ticket must survive a chat client and a double-click: {encoded}"
    );
}

#[test]
fn a_ticket_pasted_with_surrounding_whitespace_still_decodes() {
    let original = ticket(vec![direct_endpoint()]);
    let pasted = format!("\n  {}  \n", JoinTicketCodec::encode(&original));

    assert_eq!(JoinTicketCodec::decode(&pasted).expect("decodes"), original);
}

#[test]
fn encoding_is_stable_across_calls() {
    // A user who re-copies the same ticket must get the same string, or the
    // support conversation "is this the ticket you sent me?" has no answer.
    let original = ticket(vec![direct_endpoint(), relayed_endpoint()]);

    assert_eq!(
        JoinTicketCodec::encode(&original),
        JoinTicketCodec::encode(&original)
    );
}

// ------------------------------------------------------------- validity stays
// ------------------------------------------------------------- in the domain

#[test]
fn decoding_does_not_re_check_expiry_or_compatibility() {
    // The codec's job ends at "this is a well-formed ticket". Whether it may
    // be redeemed is `JoinTicket::validate`, which the application applies with
    // its own clock — checking it here as well would let the two disagree.
    let expired = ticket(vec![direct_endpoint()]);
    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&expired)).expect("decodes");

    let now = Millis::from_millis(u64::MAX - 1);
    assert!(decoded.validate(now, ProtocolVersion::CURRENT).is_err());
}

#[test]
fn an_incompatible_protocol_still_decodes_and_is_refused_by_the_domain() {
    let foreign = JoinTicket::new(
        alice(),
        vec![direct_endpoint()],
        ProtocolVersion::new(99, 0),
        Millis::MAX,
    )
    .expect("valid ticket");

    let decoded = JoinTicketCodec::decode(&JoinTicketCodec::encode(&foreign)).expect("decodes");

    assert_eq!(decoded.protocol(), ProtocolVersion::new(99, 0));
    assert!(
        decoded
            .validate(Millis::ZERO, ProtocolVersion::CURRENT)
            .is_err()
    );
}

// ------------------------------------------------------------ malformed input

#[test]
fn refuses_text_without_the_prefix() {
    assert_eq!(
        JoinTicketCodec::decode("just some text"),
        Err(JoinTicketCodecError::MissingPrefix)
    );
    assert_eq!(
        JoinTicketCodec::decode(""),
        Err(JoinTicketCodecError::MissingPrefix)
    );
}

#[test]
fn refuses_a_ticket_of_an_unknown_encoding_version() {
    let encoded = JoinTicketCodec::encode(&ticket(vec![direct_endpoint()]));
    let future = encoded.replace("distro-join-1.", "distro-join-9.");

    assert_eq!(
        JoinTicketCodec::decode(&future),
        Err(JoinTicketCodecError::MissingPrefix)
    );
}

#[test]
fn refuses_a_body_that_is_not_base64() {
    assert_eq!(
        JoinTicketCodec::decode("distro-join-1.not base64 at all!!"),
        Err(JoinTicketCodecError::NotBase64)
    );
}

#[test]
fn refuses_an_empty_body() {
    assert_eq!(
        JoinTicketCodec::decode("distro-join-1."),
        Err(JoinTicketCodecError::MalformedCbor)
    );
}

#[test]
fn refuses_every_truncation_of_a_valid_ticket_without_panicking() {
    let encoded = JoinTicketCodec::encode(&ticket(vec![direct_endpoint(), relayed_endpoint()]));

    for length in 0..encoded.len() {
        assert!(
            JoinTicketCodec::decode(&encoded[..length]).is_err(),
            "a ticket truncated to {length} characters must be refused"
        );
    }
}

#[test]
fn refuses_a_ticket_with_a_missing_field() {
    for dropped in [
        "issuer",
        "endpoints",
        "protocol_major",
        "protocol_minor",
        "expires_at_millis",
    ] {
        let fields: Vec<_> = well_formed_fields()
            .into_iter()
            .filter(|(name, _)| *name != dropped)
            .collect();

        assert!(
            matches!(
                JoinTicketCodec::decode(&body(fields)),
                Err(JoinTicketCodecError::MissingField(name)) if name == dropped
            ),
            "dropping `{dropped}` was not refused by name"
        );
    }
}

#[test]
fn refuses_a_field_of_the_wrong_type() {
    let mut fields = well_formed_fields();
    fields.retain(|(name, _)| *name != "endpoints");
    fields.push(("endpoints", Value::Text("not an array".to_owned())));

    assert_eq!(
        JoinTicketCodec::decode(&body(fields)),
        Err(JoinTicketCodecError::FieldType("endpoints"))
    );
}

#[test]
fn refuses_an_endpoint_that_is_not_an_address() {
    let mut fields = well_formed_fields();
    fields.retain(|(name, _)| *name != "endpoints");
    fields.push((
        "endpoints",
        Value::Array(vec![Value::Text("10.0.0.1:4001".to_owned())]),
    ));

    assert!(matches!(
        JoinTicketCodec::decode(&body(fields)),
        Err(JoinTicketCodecError::Endpoint(_))
    ));
}

#[test]
fn refuses_a_ticket_with_no_endpoints_because_the_domain_does() {
    let mut fields = well_formed_fields();
    fields.retain(|(name, _)| *name != "endpoints");
    fields.push(("endpoints", Value::Array(Vec::new())));

    assert!(matches!(
        JoinTicketCodec::decode(&body(fields)),
        Err(JoinTicketCodecError::Rejected(_))
    ));
}

#[test]
fn refuses_an_issuer_that_is_not_a_public_key() {
    let mut fields = well_formed_fields();
    fields.retain(|(name, _)| *name != "issuer");
    fields.push(("issuer", Value::Bytes(vec![0_u8; 16])));

    assert_eq!(
        JoinTicketCodec::decode(&body(fields)),
        Err(JoinTicketCodecError::InvalidIssuer)
    );
}

#[test]
fn refuses_a_protocol_version_out_of_range() {
    let mut fields = well_formed_fields();
    fields.retain(|(name, _)| *name != "protocol_major");
    fields.push(("protocol_major", Value::Integer(1_000_000.into())));

    assert_eq!(
        JoinTicketCodec::decode(&body(fields)),
        Err(JoinTicketCodecError::FieldRange("protocol_major"))
    );
}

#[test]
fn tolerates_a_field_a_newer_issuer_added() {
    let mut fields = well_formed_fields();
    fields.push(("issued_by_a_newer_build", Value::Bool(true)));

    assert!(JoinTicketCodec::decode(&body(fields)).is_ok());
}

#[test]
fn refuses_a_ticket_longer_than_the_cap_before_decoding_it() {
    let limits = ResourceLimits {
        max_ticket_bytes: 32,
        ..ResourceLimits::DEFAULT
    };
    let encoded = JoinTicketCodec::encode(&ticket(vec![direct_endpoint()]));

    assert!(encoded.len() > 32, "fixture must exceed the test cap");
    assert!(matches!(
        JoinTicketCodec::decode_within(&encoded, limits),
        Err(JoinTicketCodecError::TooLarge { limit: 32, .. })
    ));
}

#[test]
fn refuses_arbitrary_bytes_after_the_prefix_without_panicking() {
    for length in 0..256_usize {
        let payload: Vec<u8> = (0..length).map(|index| (index * 7 % 251) as u8).collect();
        let text = format!(
            "{}{}",
            JoinTicketCodec::PREFIX,
            URL_SAFE_NO_PAD.encode(&payload)
        );

        let _ = JoinTicketCodec::decode(&text);
    }
}
