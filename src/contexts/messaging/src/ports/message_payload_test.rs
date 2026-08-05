use shared_types::Envelope;

use crate::domain::{MessageBody, Millis, SequenceNumber};
use crate::ports::{MessagePayload, MessagePayloadError};

const CLAIMED_AT: Millis = Millis::from_millis(1_700_000_000_000);

fn body(text: &str) -> MessageBody {
    MessageBody::new(text).expect("test body is within the size limits")
}

fn payload() -> MessagePayload {
    MessagePayload::new(SequenceNumber::FIRST, CLAIMED_AT, body("hello"))
}

#[test]
fn a_payload_survives_a_round_trip_unchanged() {
    let decoded = MessagePayload::decode(&payload().encode()).expect("round trip");

    assert_eq!(decoded, payload());
}

#[test]
fn the_layout_is_pinned_by_this_test() {
    // The layout is a wire contract: peers upgrade independently and there is
    // no coordinated deploy (S2), so a change here is a protocol change and
    // must be a deliberate one.
    let bytes = MessagePayload::new(
        SequenceNumber::new(2).expect("valid"),
        Millis::from_millis(3),
        body("hi"),
    )
    .encode();

    assert_eq!(
        bytes,
        vec![
            0, 0, 0, 0, 0, 0, 0, 2, // sequence, u64 big-endian
            0, 0, 0, 0, 0, 0, 0, 3, // claimed-sent-at millis, u64 big-endian
            0, 0, 0, 2, // body length, u32 big-endian
            b'h', b'i', // body bytes, UTF-8
        ]
    );
}

#[test]
fn trailing_bytes_a_newer_peer_added_are_tolerated() {
    // S2/AC14: a same-major peer may append fields this build does not know.
    // The body is length-prefixed, so anything past it is ignored rather than
    // failing the decode — which is what keeps an older peer readable by a
    // newer one and vice versa.
    let mut bytes = payload().encode();
    bytes.extend_from_slice(b"a field from a later minor version");

    assert_eq!(MessagePayload::decode(&bytes), Ok(payload()));
}

#[test]
fn a_truncated_header_is_a_typed_error_not_a_panic() {
    // Hostile input is the normal case on an open network.
    for length in 0..MessagePayload::HEADER_BYTES {
        assert_eq!(
            MessagePayload::decode(&payload().encode()[..length]),
            Err(MessagePayloadError::TooShort)
        );
    }
}

#[test]
fn a_body_shorter_than_its_length_prefix_is_refused() {
    let mut bytes = payload().encode();
    bytes.truncate(bytes.len() - 1);

    assert_eq!(
        MessagePayload::decode(&bytes),
        Err(MessagePayloadError::BodyTruncated)
    );
}

#[test]
fn a_length_prefix_claiming_gigabytes_allocates_nothing() {
    // The cap (S6) is enforced by refusing to read past the bytes that
    // actually arrived, so a hostile length is a refusal rather than a
    // reservation.
    let mut bytes = payload().encode();
    bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());

    assert_eq!(
        MessagePayload::decode(&bytes),
        Err(MessagePayloadError::BodyTruncated)
    );
}

#[test]
fn a_zero_sequence_number_is_refused() {
    let mut bytes = payload().encode();
    bytes[..8].copy_from_slice(&0u64.to_be_bytes());

    assert!(matches!(
        MessagePayload::decode(&bytes),
        Err(MessagePayloadError::InvalidSequence(_))
    ));
}

#[test]
fn a_body_that_is_not_utf8_is_refused() {
    let mut bytes = payload().encode();
    bytes[MessagePayload::HEADER_BYTES] = 0xff;

    assert_eq!(
        MessagePayload::decode(&bytes),
        Err(MessagePayloadError::BodyNotUtf8)
    );
}

#[test]
fn an_empty_body_is_refused_by_the_domain_rule() {
    let bytes = {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u64.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes
    };

    assert!(matches!(
        MessagePayload::decode(&bytes),
        Err(MessagePayloadError::InvalidBody(_))
    ));
}

#[test]
fn an_encoded_payload_is_carried_by_an_envelope_untouched() {
    // The envelope treats the payload as opaque bytes, which is exactly what
    // lets this layout evolve without touching `shared_types`.
    let encoded = payload().encode();
    let envelope = Envelope {
        version: shared_types::ProtocolVersion::CURRENT,
        kind: shared_types::PayloadKind::DirectMessage,
        author: crate::test_peers::alice(),
        payload: encoded.clone(),
        signature: shared_types::EnvelopeSignature::new([0u8; 64]),
    };

    assert_eq!(MessagePayload::decode(&envelope.payload), Ok(payload()));
    assert_eq!(envelope.payload, encoded);
}
