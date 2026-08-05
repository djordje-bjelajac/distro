use std::collections::HashSet;

use crate::{PeerId, PeerIdError};

/// RFC 8032 §7.1 TEST 1 public key — a known-valid Ed25519 encoding.
const RFC8032_TEST1_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// RFC 8032 §7.1 TEST 2 public key — a second known-valid encoding.
const RFC8032_TEST2_PUBLIC_KEY: [u8; 32] = [
    0x3d, 0x40, 0x17, 0xc3, 0xe8, 0x43, 0x89, 0x5a, 0x92, 0xb7, 0x0a, 0xa7, 0x4d, 0x1b, 0x7e, 0xbc,
    0x9c, 0x98, 0x2c, 0xcf, 0x2e, 0xc4, 0x96, 0x8c, 0xc0, 0xcd, 0x55, 0xf1, 0x2a, 0xf4, 0x66, 0x0c,
];

/// Little-endian encoding of y = 2, which fails Edwards point decompression
/// (x² = (y² − 1)/(dy² + 1) is a non-square mod 2²⁵⁵ − 19), so it can never
/// be a valid Ed25519 public key.
const NOT_A_CURVE_POINT: [u8; 32] = [
    2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

#[test]
fn constructs_from_valid_ed25519_public_key_bytes() {
    let peer = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY)
        .expect("RFC 8032 test key must be accepted");
    assert_eq!(peer.as_bytes(), &RFC8032_TEST1_PUBLIC_KEY);
}

#[test]
fn constructs_from_a_dalek_generated_public_key() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
    let public_key_bytes = signing_key.verifying_key().to_bytes();

    let peer = PeerId::from_public_key_bytes(public_key_bytes)
        .expect("a key derived by the reference implementation must be accepted");
    assert_eq!(peer.as_bytes(), &public_key_bytes);
}

#[test]
fn rejects_bytes_that_are_not_a_curve_point() {
    let result = PeerId::from_public_key_bytes(NOT_A_CURVE_POINT);
    assert_eq!(result, Err(PeerIdError::InvalidPublicKey));
}

#[test]
fn equality_is_by_key_bytes() {
    let a = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let b = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let c = PeerId::from_public_key_bytes(RFC8032_TEST2_PUBLIC_KEY).unwrap();

    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn ordering_is_lexicographic_by_key_bytes() {
    // TEST2 key starts with 0x3d, TEST1 key with 0xd7.
    let higher = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let lower = PeerId::from_public_key_bytes(RFC8032_TEST2_PUBLIC_KEY).unwrap();

    assert!(lower < higher);
    assert_eq!(lower.cmp(&higher), std::cmp::Ordering::Less);
    assert_eq!(lower.cmp(&lower), std::cmp::Ordering::Equal);
}

#[test]
fn hashes_by_key_bytes() {
    let a = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let b = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let c = PeerId::from_public_key_bytes(RFC8032_TEST2_PUBLIC_KEY).unwrap();

    let set: HashSet<PeerId> = [a, b, c].into_iter().collect();
    assert_eq!(set.len(), 2);
}

#[test]
fn error_displays_a_diagnostic_and_implements_error() {
    let error = PeerIdError::InvalidPublicKey;
    assert_eq!(
        error.to_string(),
        "bytes are not a valid Ed25519 public key"
    );
    let _: &dyn std::error::Error = &error;
}
