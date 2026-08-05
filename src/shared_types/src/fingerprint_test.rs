use crate::{Fingerprint, PeerId};

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

/// SHA-256 of the TEST 1 public key, computed with an independent
/// implementation (Python `hashlib`), not with this crate.
const TEST1_KEY_SHA256: [u8; 32] = [
    0x21, 0xfe, 0x31, 0xdf, 0xa1, 0x54, 0xa2, 0x61, 0x62, 0x6b, 0xf8, 0x54, 0x04, 0x6f, 0xd2, 0x27,
    0x1b, 0x7b, 0xed, 0x4b, 0x6a, 0xbe, 0x45, 0xaa, 0x58, 0x87, 0x7e, 0xf4, 0x7f, 0x97, 0x21, 0xb9,
];

fn peer(bytes: [u8; 32]) -> PeerId {
    PeerId::from_public_key_bytes(bytes).unwrap()
}

#[test]
fn digest_is_sha256_of_the_public_key_bytes() {
    let fingerprint = Fingerprint::of(&peer(RFC8032_TEST1_PUBLIC_KEY));
    assert_eq!(fingerprint.as_bytes(), &TEST1_KEY_SHA256);
}

#[test]
fn is_deterministic_for_the_same_peer() {
    let a = Fingerprint::of(&peer(RFC8032_TEST1_PUBLIC_KEY));
    let b = Fingerprint::of(&peer(RFC8032_TEST1_PUBLIC_KEY));
    assert_eq!(a, b);
}

#[test]
fn differs_between_different_peers() {
    let a = Fingerprint::of(&peer(RFC8032_TEST1_PUBLIC_KEY));
    let b = Fingerprint::of(&peer(RFC8032_TEST2_PUBLIC_KEY));
    assert_ne!(a, b);
}

/// Stability pin: the exact rendering for a fixed key. If this test breaks,
/// the fingerprint format changed — which silently invalidates every
/// fingerprint users have already compared out-of-band. Never update this
/// expectation without a deliberate, versioned format decision.
#[test]
fn rendering_is_pinned_for_a_fixed_key() {
    let fingerprint = Fingerprint::of(&peer(RFC8032_TEST1_PUBLIC_KEY));
    assert_eq!(
        fingerprint.to_string(),
        "21fe 31df a154 a261 626b f854 046f d227"
    );
}

#[test]
fn rendering_shape_is_eight_groups_of_four_lowercase_hex_chars() {
    let rendered = Fingerprint::of(&peer(RFC8032_TEST2_PUBLIC_KEY)).to_string();

    let groups: Vec<&str> = rendered.split(' ').collect();
    assert_eq!(groups.len(), 8);
    for group in groups {
        assert_eq!(group.len(), 4);
        assert!(
            group
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}
