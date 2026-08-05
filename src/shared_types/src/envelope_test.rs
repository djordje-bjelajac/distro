use crate::{Compatibility, Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

/// RFC 8032 §7.1 TEST 1 public key.
const RFC8032_TEST1_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// RFC 8032 §7.1 TEST 2 public key.
const RFC8032_TEST2_PUBLIC_KEY: [u8; 32] = [
    0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b, 0x7e, 0xbc,
    0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c,
];

fn author() -> PeerId {
    PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap()
}

fn envelope() -> Envelope {
    Envelope {
        version: ProtocolVersion::new(1, 0),
        kind: PayloadKind::DirectMessage,
        author: author(),
        payload: b"hi".to_vec(),
        signature: EnvelopeSignature::new([0u8; 64]),
    }
}

#[test]
fn signable_bytes_is_deterministic() {
    assert_eq!(envelope().signable_bytes(), envelope().signable_bytes());
}

#[test]
fn signable_bytes_excludes_the_signature() {
    let unsigned = envelope();
    let signed = Envelope {
        signature: EnvelopeSignature::new([0xff; 64]),
        ..envelope()
    };
    assert_eq!(unsigned.signable_bytes(), signed.signable_bytes());
}

#[test]
fn signable_bytes_changes_when_major_version_changes() {
    let changed = Envelope {
        version: ProtocolVersion::new(2, 0),
        ..envelope()
    };
    assert_ne!(envelope().signable_bytes(), changed.signable_bytes());
}

#[test]
fn signable_bytes_changes_when_minor_version_changes() {
    let changed = Envelope {
        version: ProtocolVersion::new(1, 1),
        ..envelope()
    };
    assert_ne!(envelope().signable_bytes(), changed.signable_bytes());
}

#[test]
fn signable_bytes_changes_when_kind_changes() {
    let changed = Envelope {
        kind: PayloadKind::BroadcastMessage,
        ..envelope()
    };
    assert_ne!(envelope().signable_bytes(), changed.signable_bytes());
}

#[test]
fn signable_bytes_changes_when_author_changes() {
    let changed = Envelope {
        author: PeerId::from_public_key_bytes(RFC8032_TEST2_PUBLIC_KEY).unwrap(),
        ..envelope()
    };
    assert_ne!(envelope().signable_bytes(), changed.signable_bytes());
}

#[test]
fn signable_bytes_changes_when_payload_changes() {
    let changed = Envelope {
        payload: b"ho".to_vec(),
        ..envelope()
    };
    assert_ne!(envelope().signable_bytes(), changed.signable_bytes());
}

/// Layout pin: the exact signing input for a fixed envelope, cross-checked
/// against an independent implementation (Python). If this test breaks, the
/// canonical signing layout changed — which invalidates signature
/// verification between app versions. Never update this expectation without
/// a major protocol version bump.
#[test]
fn signable_bytes_layout_is_pinned() {
    let mut expected: Vec<u8> = Vec::new();
    expected.extend_from_slice(&[0x00, 0x01]); // version.major = 1, u16 BE
    expected.extend_from_slice(&[0x00, 0x00]); // version.minor = 0, u16 BE
    expected.extend_from_slice(&[0x00, 0x00]); // kind code = 0 (DirectMessage), u16 BE
    expected.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]); // author length = 32, u32 BE
    expected.extend_from_slice(&RFC8032_TEST1_PUBLIC_KEY); // author key bytes
    expected.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // payload length = 2, u32 BE
    expected.extend_from_slice(b"hi"); // payload bytes

    assert_eq!(envelope().signable_bytes(), expected);
    assert_eq!(expected.len(), 48);
}

#[test]
fn signable_bytes_of_empty_payload_carries_a_zero_length_prefix() {
    let empty = Envelope {
        payload: Vec::new(),
        ..envelope()
    };
    let bytes = empty.signable_bytes();
    assert_eq!(bytes.len(), 46);
    assert_eq!(&bytes[42..46], &[0, 0, 0, 0]);
}

#[test]
fn compatibility_delegates_to_the_version_rule() {
    let supported = ProtocolVersion::CURRENT;

    assert_eq!(envelope().compatibility(&supported), Compatibility::Accept);

    let newer_minor = Envelope {
        version: ProtocolVersion::new(supported.major, supported.minor + 1),
        ..envelope()
    };
    assert_eq!(
        newer_minor.compatibility(&supported),
        Compatibility::Tolerate
    );

    let other_major = Envelope {
        version: ProtocolVersion::new(supported.major + 1, 0),
        ..envelope()
    };
    assert_eq!(other_major.compatibility(&supported), Compatibility::Reject);
}
