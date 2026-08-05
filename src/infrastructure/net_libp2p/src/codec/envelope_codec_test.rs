use ciborium::value::Value;
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::codec::{CodecDiagnostics, EnvelopeCodec, EnvelopeCodecError};
use crate::limits::ResourceLimits;
use crate::test_peers::alice;

const SUPPORTED: ProtocolVersion = ProtocolVersion { major: 1, minor: 4 };

fn codec() -> EnvelopeCodec {
    EnvelopeCodec::new(SUPPORTED, ResourceLimits::DEFAULT, CodecDiagnostics::new())
}

fn signature() -> EnvelopeSignature {
    let mut bytes = [0_u8; EnvelopeSignature::LENGTH];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    EnvelopeSignature::new(bytes)
}

fn envelope(version: ProtocolVersion, kind: PayloadKind) -> Envelope {
    Envelope {
        version,
        kind,
        author: alice(),
        payload: b"a payload the codec never reads".to_vec(),
        signature: signature(),
    }
}

/// A frame built field by field, so a test can add, drop, or corrupt exactly
/// one thing.
fn frame(fields: Vec<(&str, Value)>) -> Vec<u8> {
    let map = Value::Map(
        fields
            .into_iter()
            .map(|(name, value)| (Value::Text(name.to_owned()), value))
            .collect(),
    );
    let mut bytes = Vec::new();
    ciborium::into_writer(&map, &mut bytes).expect("fixture encodes");
    bytes
}

fn well_formed_fields(major: u16, minor: u16, kind: u16) -> Vec<(&'static str, Value)> {
    vec![
        ("version_major", Value::Integer(major.into())),
        ("version_minor", Value::Integer(minor.into())),
        ("kind", Value::Integer(kind.into())),
        ("author", Value::Bytes(alice().as_bytes().to_vec())),
        ("payload", Value::Bytes(b"body".to_vec())),
        ("signature", Value::Bytes(signature().as_bytes().to_vec())),
    ]
}

// ----------------------------------------------------------------- round trip

#[test]
fn round_trips_a_known_envelope() {
    let codec = codec();
    let original = envelope(SUPPORTED, PayloadKind::DirectMessage);

    let bytes = codec.encode(&original).expect("encodes");
    let decoded = codec.decode(&bytes).expect("decodes");

    assert_eq!(decoded, original);
}

#[test]
fn round_trips_every_assigned_payload_kind() {
    let codec = codec();

    for kind in [
        PayloadKind::DirectMessage,
        PayloadKind::BroadcastMessage,
        PayloadKind::Heartbeat,
    ] {
        let original = envelope(SUPPORTED, kind);
        let decoded = codec
            .decode(&codec.encode(&original).expect("encodes"))
            .expect("decodes");

        assert_eq!(decoded.kind, kind);
        assert_eq!(decoded, original);
    }
}

#[test]
fn round_trips_an_empty_payload() {
    let codec = codec();
    let mut original = envelope(SUPPORTED, PayloadKind::Heartbeat);
    original.payload = Vec::new();

    let decoded = codec
        .decode(&codec.encode(&original).expect("encodes"))
        .expect("decodes");

    assert_eq!(decoded, original);
}

#[test]
fn forwards_an_unknown_payload_kind_with_its_original_code() {
    let codec = codec();
    let original = envelope(SUPPORTED, PayloadKind::Unknown(4_242));

    let decoded = codec
        .decode(&codec.encode(&original).expect("encodes"))
        .expect("decodes");

    assert_eq!(decoded.kind, PayloadKind::Unknown(4_242));
    assert_eq!(decoded.kind.code(), 4_242);
}

#[test]
fn the_codec_never_touches_the_signing_input() {
    // The signature is carried opaquely: a re-encode must leave
    // `signable_bytes` — the layout `shared_types` pins — untouched, because
    // that is what lets peers with different codecs verify each other.
    let codec = codec();
    let original = envelope(SUPPORTED, PayloadKind::BroadcastMessage);
    let before = original.signable_bytes();

    let decoded = codec
        .decode(&codec.encode(&original).expect("encodes"))
        .expect("decodes");

    assert_eq!(decoded.signable_bytes(), before);
    assert_eq!(decoded.signature, original.signature);
}

// ------------------------------------------------- S2 tolerance / rejection

#[test]
fn s2_same_major_lower_minor_is_accepted_without_diagnostics() {
    let codec = codec();
    let bytes = frame(well_formed_fields(1, 0, 0));

    let decoded = codec.decode(&bytes).expect("accepted");

    assert_eq!(decoded.version, ProtocolVersion::new(1, 0));
    assert_eq!(codec.diagnostics().tolerated_minor(), 0);
    assert_eq!(codec.diagnostics().rejected_major(), 0);
}

#[test]
fn s2_same_major_same_minor_is_accepted_without_diagnostics() {
    let codec = codec();
    let bytes = frame(well_formed_fields(1, 4, 1));

    assert!(codec.decode(&bytes).is_ok());
    assert_eq!(codec.diagnostics().tolerated_minor(), 0);
}

#[test]
fn s2_same_major_higher_minor_is_tolerated_and_counted() {
    let codec = codec();
    let bytes = frame(well_formed_fields(1, 9, 0));

    let decoded = codec.decode(&bytes).expect("tolerated, not refused");

    assert_eq!(decoded.version, ProtocolVersion::new(1, 9));
    assert_eq!(codec.diagnostics().tolerated_minor(), 1);
    assert_eq!(codec.diagnostics().rejected_major(), 0);
}

#[test]
fn s2_same_major_unknown_fields_are_ignored_and_counted() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 4, 0);
    fields.push(("a_field_from_the_future", Value::Text("hello".to_owned())));
    fields.push(("another_one", Value::Integer(7.into())));

    let decoded = codec.decode(&frame(fields)).expect("ignored, not refused");

    assert_eq!(decoded.author, alice());
    assert_eq!(codec.diagnostics().unknown_fields(), 2);
    assert_eq!(codec.diagnostics().malformed_frames(), 0);
}

#[test]
fn s2_a_newer_minor_carrying_unknown_fields_is_tolerated_and_both_are_counted() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 7, 0);
    fields.push(("shiny", Value::Bool(true)));

    assert!(codec.decode(&frame(fields)).is_ok());

    assert_eq!(codec.diagnostics().tolerated_minor(), 1);
    assert_eq!(codec.diagnostics().unknown_fields(), 1);
}

#[test]
fn s2_an_unknown_payload_kind_is_tolerated_and_counted() {
    let codec = codec();
    let bytes = frame(well_formed_fields(1, 4, 9_001));

    let decoded = codec.decode(&bytes).expect("tolerated, not refused");

    assert_eq!(decoded.kind, PayloadKind::Unknown(9_001));
    assert_eq!(codec.diagnostics().unknown_payload_kinds(), 1);
    assert_eq!(codec.diagnostics().malformed_frames(), 0);
}

#[test]
fn s2_a_higher_major_is_rejected_with_a_reason() {
    let codec = codec();
    let bytes = frame(well_formed_fields(2, 0, 0));

    let error = codec.decode(&bytes).expect_err("rejected");

    assert_eq!(
        error,
        EnvelopeCodecError::IncompatibleMajor {
            received: ProtocolVersion::new(2, 0),
            supported: SUPPORTED,
        }
    );
    assert!(
        error.to_string().contains("incompatible wire format"),
        "the rejection must state its reason: {error}"
    );
    assert_eq!(codec.diagnostics().rejected_major(), 1);
}

#[test]
fn s2_a_lower_major_is_rejected_with_a_reason() {
    let codec = codec();
    let bytes = frame(well_formed_fields(0, 9, 0));

    let error = codec.decode(&bytes).expect_err("rejected");

    assert!(matches!(
        error,
        EnvelopeCodecError::IncompatibleMajor { .. }
    ));
    assert_eq!(codec.diagnostics().rejected_major(), 1);
}

#[test]
fn s2_a_rejected_major_is_decided_before_anything_else_is_read() {
    // Every field but the version is unreadable. The envelope must still be
    // refused for its major version, because that is the honest reason — a
    // build that cannot read the format cannot judge the contents.
    let codec = codec();
    let bytes = frame(vec![
        ("version_major", Value::Integer(3.into())),
        ("version_minor", Value::Integer(0.into())),
        ("kind", Value::Text("not an integer".to_owned())),
        ("author", Value::Bool(false)),
    ]);

    let error = codec.decode(&bytes).expect_err("rejected");

    assert!(matches!(
        error,
        EnvelopeCodecError::IncompatibleMajor { .. }
    ));
    assert_eq!(codec.diagnostics().rejected_major(), 1);
    assert_eq!(codec.diagnostics().malformed_frames(), 0);
}

// ------------------------------------------------------------ S6 size caps

#[test]
fn an_oversize_frame_is_refused_before_deserialization() {
    let limits = ResourceLimits {
        max_envelope_bytes: 64,
        ..ResourceLimits::DEFAULT
    };
    let codec = EnvelopeCodec::new(SUPPORTED, limits, CodecDiagnostics::new());

    // Deliberately not valid CBOR: if the size check did not run first, the
    // error would be `MalformedCbor` and the bytes would have been parsed.
    let hostile = vec![0xff_u8; 65];

    let error = codec.decode(&hostile).expect_err("refused");

    assert_eq!(
        error,
        EnvelopeCodecError::TooLarge {
            bytes: 65,
            limit: 64
        }
    );
    assert_eq!(codec.diagnostics().oversize_frames(), 1);
    assert_eq!(
        codec.diagnostics().malformed_frames(),
        0,
        "nothing was deserialized, so nothing was malformed"
    );
}

#[test]
fn a_frame_exactly_at_the_cap_is_still_looked_at() {
    let bytes = frame(well_formed_fields(1, 4, 0));
    let limits = ResourceLimits {
        max_envelope_bytes: bytes.len(),
        ..ResourceLimits::DEFAULT
    };
    let exact = EnvelopeCodec::new(SUPPORTED, limits, CodecDiagnostics::new());

    assert!(exact.decode(&bytes).is_ok());
    assert_eq!(exact.diagnostics().oversize_frames(), 0);
}

#[test]
fn encoding_refuses_a_payload_over_the_cap() {
    let limits = ResourceLimits {
        max_envelope_bytes: 128,
        ..ResourceLimits::DEFAULT
    };
    let codec = EnvelopeCodec::new(SUPPORTED, limits, CodecDiagnostics::new());
    let mut oversize = envelope(SUPPORTED, PayloadKind::DirectMessage);
    oversize.payload = vec![0_u8; 512];

    assert!(matches!(
        codec.encode(&oversize),
        Err(EnvelopeCodecError::PayloadTooLarge { .. })
    ));
}

// ----------------------------------------------------------- malformed input

#[test]
fn refuses_bytes_that_are_not_cbor() {
    let codec = codec();

    assert_eq!(
        codec.decode(&[0xff, 0xfe, 0xfd]),
        Err(EnvelopeCodecError::MalformedCbor)
    );
    assert_eq!(codec.diagnostics().malformed_frames(), 1);
}

#[test]
fn refuses_an_empty_frame() {
    let codec = codec();

    assert_eq!(codec.decode(&[]), Err(EnvelopeCodecError::MalformedCbor));
}

#[test]
fn refuses_cbor_that_is_not_a_map() {
    let codec = codec();
    let mut bytes = Vec::new();
    ciborium::into_writer(&Value::Array(vec![Value::Integer(1.into())]), &mut bytes)
        .expect("fixture encodes");

    assert_eq!(codec.decode(&bytes), Err(EnvelopeCodecError::NotAMap));
}

#[test]
fn refuses_a_frame_with_a_missing_required_field() {
    let codec = codec();

    for dropped in [
        "version_major",
        "version_minor",
        "kind",
        "author",
        "payload",
        "signature",
    ] {
        let fields: Vec<_> = well_formed_fields(1, 4, 0)
            .into_iter()
            .filter(|(name, _)| *name != dropped)
            .collect();

        let error = codec.decode(&frame(fields)).expect_err("refused");

        assert!(
            matches!(error, EnvelopeCodecError::MissingField(name) if name == dropped),
            "dropping `{dropped}` produced {error}"
        );
    }
}

#[test]
fn refuses_a_field_of_the_wrong_type() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 4, 0);
    fields.retain(|(name, _)| *name != "author");
    fields.push(("author", Value::Text("not bytes".to_owned())));

    assert_eq!(
        codec.decode(&frame(fields)),
        Err(EnvelopeCodecError::FieldType("author"))
    );
}

#[test]
fn refuses_a_version_field_out_of_range() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 4, 0);
    fields.retain(|(name, _)| *name != "version_minor");
    fields.push(("version_minor", Value::Integer(70_000.into())));

    assert_eq!(
        codec.decode(&frame(fields)),
        Err(EnvelopeCodecError::FieldRange("version_minor"))
    );
}

#[test]
fn refuses_an_author_that_is_not_a_valid_public_key() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 4, 0);
    fields.retain(|(name, _)| *name != "author");
    fields.push(("author", Value::Bytes(vec![0_u8; PeerId::LENGTH - 1])));

    assert_eq!(
        codec.decode(&frame(fields)),
        Err(EnvelopeCodecError::InvalidAuthor)
    );
}

#[test]
fn refuses_a_signature_of_the_wrong_length() {
    let codec = codec();
    let mut fields = well_formed_fields(1, 4, 0);
    fields.retain(|(name, _)| *name != "signature");
    fields.push(("signature", Value::Bytes(vec![7_u8; 32])));

    assert_eq!(
        codec.decode(&frame(fields)),
        Err(EnvelopeCodecError::InvalidSignature)
    );
}

#[test]
fn refuses_a_truncated_frame_without_panicking() {
    let codec = codec();
    let bytes = frame(well_formed_fields(1, 4, 0));

    for length in 1..bytes.len() {
        let error = codec.decode(&bytes[..length]);
        assert!(
            error.is_err(),
            "a frame truncated to {length} bytes must be refused"
        );
    }
}
