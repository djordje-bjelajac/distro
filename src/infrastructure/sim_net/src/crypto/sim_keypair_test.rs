use ed25519_dalek::{Signature, VerifyingKey};

use crate::crypto::SimKeypair;

const SEED: u64 = 0x5EED;

#[test]
fn a_derived_keypair_is_the_same_in_every_run() {
    let first = SimKeypair::derived(SEED, "alice");
    let second = SimKeypair::derived(SEED, "alice");

    assert_eq!(first.peer(), second.peer());
}

#[test]
fn different_labels_are_different_peers() {
    let alice = SimKeypair::derived(SEED, "alice");
    let bob = SimKeypair::derived(SEED, "bob");

    assert_ne!(alice.peer(), bob.peer());
}

#[test]
fn a_different_seed_gives_the_same_label_a_different_identity() {
    let alice = SimKeypair::derived(1, "alice");
    let other_alice = SimKeypair::derived(2, "alice");

    assert_ne!(alice.peer(), other_alice.peer());
}

#[test]
fn signatures_verify_under_the_public_key_that_is_the_peer_id() {
    // The property the whole crypto seam rests on: no key lookup is ever
    // needed, because the identity *is* the verifying key (invariant 1).
    let keypair = SimKeypair::derived(SEED, "alice");
    let message = b"the signable bytes of some envelope";

    let signature = keypair.sign_bytes(message);
    let key = VerifyingKey::from_bytes(keypair.peer().as_bytes()).expect("PeerId is a valid key");

    assert!(
        key.verify_strict(message, &Signature::from_bytes(signature.as_bytes()))
            .is_ok()
    );
}

#[test]
fn signing_is_deterministic_so_a_trace_stays_byte_identical() {
    // Ed25519 signatures are deterministic by specification; asserting it here
    // pins the property a recorded trace depends on.
    let keypair = SimKeypair::derived(SEED, "alice");

    assert_eq!(keypair.sign_bytes(b"same"), keypair.sign_bytes(b"same"));
}

#[test]
fn a_signature_does_not_verify_over_different_bytes() {
    let keypair = SimKeypair::derived(SEED, "alice");
    let signature = keypair.sign_bytes(b"original");
    let key = VerifyingKey::from_bytes(keypair.peer().as_bytes()).expect("PeerId is a valid key");

    assert!(
        key.verify_strict(b"tampered", &Signature::from_bytes(signature.as_bytes()))
            .is_err()
    );
}

#[test]
fn the_debug_rendering_carries_no_secret_material() {
    let keypair = SimKeypair::derived(SEED, "alice");
    let rendered = format!("{keypair:?}");

    assert!(rendered.contains("SimKeypair"));
    assert!(
        !rendered.contains("signing"),
        "the signing key leaked into Debug: {rendered}"
    );
}
