use shared_types::{EnvelopeSignature, PayloadKind, ProtocolVersion};

use crate::domain::UnsignedEnvelope;
use crate::test_peers;

fn draft() -> UnsignedEnvelope {
    UnsignedEnvelope::draft(
        test_peers::alice(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"hello".to_vec(),
    )
}

#[test]
fn exposes_the_drafted_fields() {
    let draft = draft();

    assert_eq!(draft.author(), test_peers::alice());
    assert_eq!(draft.version(), ProtocolVersion::CURRENT);
    assert_eq!(draft.kind(), PayloadKind::DirectMessage);
    assert_eq!(draft.payload(), b"hello");
}

#[test]
fn signable_bytes_equal_those_of_the_envelope_the_draft_becomes() {
    let draft = draft();
    let expected = draft.signable_bytes();

    let signed = draft.into_signed(EnvelopeSignature::new([7u8; EnvelopeSignature::LENGTH]));

    assert_eq!(
        signed.signable_bytes(),
        expected,
        "what was signed must be what every receiver verifies"
    );
}

#[test]
fn signable_bytes_do_not_depend_on_the_signature() {
    let expected = draft().signable_bytes();

    for filler in [0u8, 7, 255] {
        let signed =
            draft().into_signed(EnvelopeSignature::new([filler; EnvelopeSignature::LENGTH]));
        assert_eq!(signed.signable_bytes(), expected);
    }
}

#[test]
fn into_signed_preserves_every_field_and_applies_the_signature() {
    let signature = EnvelopeSignature::new([3u8; EnvelopeSignature::LENGTH]);

    let signed = draft().into_signed(signature);

    assert_eq!(signed.author, test_peers::alice());
    assert_eq!(signed.version, ProtocolVersion::CURRENT);
    assert_eq!(signed.kind, PayloadKind::DirectMessage);
    assert_eq!(signed.payload, b"hello".to_vec());
    assert_eq!(signed.signature, signature);
}

#[test]
fn distinct_drafts_produce_distinct_signable_bytes() {
    let other_payload = UnsignedEnvelope::draft(
        test_peers::alice(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"hell".to_vec(),
    );
    let other_author = UnsignedEnvelope::draft(
        test_peers::bob(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"hello".to_vec(),
    );
    let other_kind = UnsignedEnvelope::draft(
        test_peers::alice(),
        ProtocolVersion::CURRENT,
        PayloadKind::BroadcastMessage,
        b"hello".to_vec(),
    );

    let baseline = draft().signable_bytes();
    assert_ne!(other_payload.signable_bytes(), baseline);
    assert_ne!(other_author.signable_bytes(), baseline);
    assert_ne!(other_kind.signable_bytes(), baseline);
}
