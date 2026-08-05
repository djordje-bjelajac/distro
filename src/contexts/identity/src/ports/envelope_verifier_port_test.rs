use shared_types::{Envelope, EnvelopeSignature, PayloadKind};

use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::port_fakes::{CheckingVerifier, RecordingSigner, UnavailableVerifier};
use crate::ports::{
    EnvelopeSignerPort, EnvelopeVerifierError, EnvelopeVerifierPort, SignatureVerdict,
};
use crate::test_peers;

fn identity_of(peer: shared_types::PeerId) -> LocalIdentity {
    let (identity, _) =
        LocalIdentity::initialize(peer, DisplayName::new("fixture").expect("valid fixture"));
    identity
}

/// The whole loop the canvas cares about: the aggregate drafts, the signer
/// port signs, the verifier port accepts — with the signing input proven to be
/// the envelope's own `signable_bytes()` (AC6, invariant 4).
#[test]
fn an_envelope_sealed_by_its_author_verifies() {
    let alice = identity_of(test_peers::alice());
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let verifier: &dyn EnvelopeVerifierPort = &CheckingVerifier;

    let envelope = signer
        .seal(alice.draft_envelope(PayloadKind::DirectMessage, b"hello".to_vec()))
        .expect("the fake signer succeeds");

    assert_eq!(
        signer.signed_inputs(),
        vec![envelope.signable_bytes()],
        "the bytes signed are exactly the bytes verified"
    );
    let verdict = verifier
        .verify(&envelope)
        .expect("the verifier is available");
    assert_eq!(verdict, SignatureVerdict::Valid);
    assert!(verdict.is_valid());
}

#[test]
fn a_tampered_payload_is_rejected() {
    let alice = identity_of(test_peers::alice());
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(alice.draft_envelope(PayloadKind::DirectMessage, b"pay alice".to_vec()))
        .expect("the fake signer succeeds");

    let tampered = Envelope {
        payload: b"pay mallory".to_vec(),
        ..envelope
    };

    let verdict = CheckingVerifier
        .verify(&tampered)
        .expect("the verifier is available");
    assert_eq!(verdict, SignatureVerdict::Invalid);
    assert!(!verdict.is_valid());
}

#[test]
fn an_envelope_signed_with_another_peers_key_is_rejected() {
    // Mallory drafts as herself but signs with a key that is not hers, and
    // separately claims Alice as author: neither forgery verifies.
    let alice = identity_of(test_peers::alice());
    let mallorys_signer = RecordingSigner::holding_key_of(test_peers::bob());

    let forged = mallorys_signer
        .seal(alice.draft_envelope(PayloadKind::DirectMessage, b"trust me".to_vec()))
        .expect("the fake signer succeeds");

    assert_eq!(forged.author, test_peers::alice());
    assert_eq!(
        CheckingVerifier
            .verify(&forged)
            .expect("the verifier is available"),
        SignatureVerdict::Invalid,
        "the author field is only a claim until the signature backs it"
    );
}

#[test]
fn a_garbage_signature_is_a_verdict_not_a_panic() {
    let alice = identity_of(test_peers::alice());
    let envelope = alice
        .draft_envelope(PayloadKind::BroadcastMessage, b"noise".to_vec())
        .into_signed(EnvelopeSignature::new([0xff; EnvelopeSignature::LENGTH]));

    assert_eq!(
        CheckingVerifier
            .verify(&envelope)
            .expect("the verifier is available"),
        SignatureVerdict::Invalid
    );
}

#[test]
fn an_unavailable_verifier_reports_a_typed_error_never_a_verdict() {
    let alice = identity_of(test_peers::alice());
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(alice.draft_envelope(PayloadKind::DirectMessage, b"hello".to_vec()))
        .expect("the fake signer succeeds");
    let port: &dyn EnvelopeVerifierPort = &UnavailableVerifier;

    let result = port.verify(&envelope);

    assert_eq!(result, Err(EnvelopeVerifierError::VerifierUnavailable));
    assert!(
        result.is_err(),
        "an un-performed check must never be readable as Valid"
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let error = EnvelopeVerifierError::VerifierUnavailable;

    assert_eq!(
        error.to_string(),
        "signature verifier is unavailable; authenticity is unknown"
    );
    let _: &dyn std::error::Error = &error;
}
