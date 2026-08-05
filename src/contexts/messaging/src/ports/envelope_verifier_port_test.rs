use shared_types::{EnvelopeSignature, PayloadKind, ProtocolVersion};

use crate::ports::port_fakes::{CheckingVerifier, RecordingSigner, UnavailableVerifier};
use crate::ports::{
    EnvelopeSignerPort, EnvelopeVerifierError, EnvelopeVerifierPort, SignatureVerdict,
    UnsignedEnvelope,
};
use crate::test_peers;

fn signed_by_alice() -> shared_types::Envelope {
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    signer
        .seal(UnsignedEnvelope::draft(
            test_peers::alice(),
            ProtocolVersion::CURRENT,
            PayloadKind::BroadcastMessage,
            b"hello everyone".to_vec(),
        ))
        .expect("the local key is available")
}

#[test]
fn the_port_is_object_safe_so_one_verifier_can_be_shared() {
    let verifier = CheckingVerifier;
    let port: &dyn EnvelopeVerifierPort = &verifier;

    assert_eq!(port.verify(&signed_by_alice()), Ok(SignatureVerdict::Valid));
}

#[test]
fn a_genuine_signature_verifies_against_the_envelopes_own_author() {
    assert_eq!(
        CheckingVerifier.verify(&signed_by_alice()),
        Ok(SignatureVerdict::Valid)
    );
}

#[test]
fn a_tampered_payload_does_not_verify() {
    // Invariant 10: content that fails verification never reaches a read
    // model, and this is how the application finds out.
    let mut envelope = signed_by_alice();
    envelope.payload = b"hello everyone!".to_vec();

    assert_eq!(
        CheckingVerifier.verify(&envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn an_envelope_re_attributed_to_another_peer_does_not_verify() {
    // Invariant 4: the author is whoever the signature verifies for. Swapping
    // the author field cannot make someone else the author.
    let mut envelope = signed_by_alice();
    envelope.author = test_peers::bob();

    assert_eq!(
        CheckingVerifier.verify(&envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn a_forged_signature_is_a_verdict_not_an_error() {
    // Hostile input is the normal case on an open network, so a bad signature
    // must never panic or abort — it is data the caller counts (AC6).
    let mut envelope = signed_by_alice();
    envelope.signature = EnvelopeSignature::new([0xab; EnvelopeSignature::LENGTH]);

    assert_eq!(
        CheckingVerifier.verify(&envelope),
        Ok(SignatureVerdict::Invalid)
    );
}

#[test]
fn a_verifier_that_cannot_run_is_not_a_verdict() {
    // "Could not check" must never be readable as "valid"; the distinction is
    // what lets diagnostics tell a forgery from a broken verifier.
    let error = UnavailableVerifier
        .verify(&signed_by_alice())
        .expect_err("an unavailable verifier reports an error");

    assert_eq!(error, EnvelopeVerifierError::VerifierUnavailable);
}

#[test]
fn a_verdict_states_plainly_whether_it_is_valid() {
    assert!(SignatureVerdict::Valid.is_valid());
    assert!(!SignatureVerdict::Invalid.is_valid());
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        EnvelopeVerifierError::VerifierUnavailable.to_string(),
        "signature verifier is unavailable; authenticity is unknown"
    );
}
