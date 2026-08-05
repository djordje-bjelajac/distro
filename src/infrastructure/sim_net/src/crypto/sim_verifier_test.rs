use std::sync::Arc;

use messaging::ports::{
    EnvelopeSignerPort, EnvelopeVerifierPort, SignatureVerdict, UnsignedEnvelope,
};
use shared_types::{EnvelopeSignature, PayloadKind, ProtocolVersion};

use crate::crypto::{SimKeypair, SimSigner, SimVerifier};

const SEED: u64 = 0x_1E_F1;

fn sealed_by(keypair: &Arc<SimKeypair>) -> shared_types::Envelope {
    SimSigner::new(Arc::clone(keypair))
        .seal(UnsignedEnvelope::draft(
            keypair.peer(),
            ProtocolVersion::CURRENT,
            PayloadKind::BroadcastMessage,
            b"hello".to_vec(),
        ))
        .expect("the signer holds this author's key")
}

#[test]
fn a_genuine_signature_is_valid_for_both_contexts() {
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let envelope = sealed_by(&keypair);

    assert_eq!(
        EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(SignatureVerdict::Valid)
    );
    assert_eq!(
        identity::ports::EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(identity::ports::SignatureVerdict::Valid)
    );
}

#[test]
fn a_tampered_payload_invalidates_the_signature() {
    // AC6 at its sharpest: the bytes signed are the envelope's signable bytes,
    // so changing the payload after signing is detectable without any lookup.
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let mut envelope = sealed_by(&keypair);
    envelope.payload = b"hello!".to_vec();

    assert_eq!(
        EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn a_forged_author_invalidates_the_signature() {
    // Invariant 4: the author is whoever the signature verifies for. Relabelling
    // the envelope with another peer's identity cannot make that peer the author.
    let alice = Arc::new(SimKeypair::derived(SEED, "alice"));
    let bob = SimKeypair::derived(SEED, "bob");
    let mut envelope = sealed_by(&alice);
    envelope.author = bob.peer();

    assert_eq!(
        EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn a_corrupted_signature_is_invalid_rather_than_a_panic() {
    // Hostile input is the normal case on an open network; a verifier that
    // panicked on a malformed signature would take the peer down with it.
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let mut envelope = sealed_by(&keypair);
    envelope.signature = EnvelopeSignature::new([0xFF; EnvelopeSignature::LENGTH]);

    assert_eq!(
        EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn a_flipped_signature_bit_is_invalid() {
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let mut envelope = sealed_by(&keypair);
    let mut bytes = *envelope.signature.as_bytes();
    bytes[0] ^= 0x01;
    envelope.signature = EnvelopeSignature::new(bytes);

    assert_eq!(
        EnvelopeVerifierPort::verify(&SimVerifier, &envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn the_verifier_never_reports_that_it_could_not_run() {
    // The error variant means "authenticity unknown", which a caller must never
    // read as valid. This verifier can always run, so it never says so.
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let envelope = sealed_by(&keypair);

    assert!(EnvelopeVerifierPort::verify(&SimVerifier, &envelope).is_ok());
}
