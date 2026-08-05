use shared_types::{Envelope, EnvelopeSignature, PayloadKind, ProtocolVersion};

use crate::ports::port_fakes::{CheckingVerifier, RecordingSigner, UnavailableVerifier};
use crate::ports::{EnvelopeSignerPort, EnvelopeVerifierError, UnsignedEnvelope, VerifiedAuthor};
use crate::test_peers;

fn signed_by_bob() -> Envelope {
    let signer = RecordingSigner::holding_key_of(test_peers::bob());

    signer
        .seal(UnsignedEnvelope::draft(
            test_peers::bob(),
            ProtocolVersion::CURRENT,
            PayloadKind::DirectMessage,
            b"payload".to_vec(),
        ))
        .expect("the fake signer holds bob's key")
}

#[test]
fn a_valid_signature_yields_the_envelopes_author() {
    let envelope = signed_by_bob();

    let attested = VerifiedAuthor::attest(&CheckingVerifier, &envelope)
        .expect("the verifier is available")
        .expect("the signature verifies");

    assert_eq!(attested.into_peer(), test_peers::bob());
}

#[test]
fn an_invalid_signature_yields_no_author_at_all() {
    // Invariant 4: the author is whoever the signature verifies for. With no
    // verifying signature there is no author, and the type says so by not
    // existing rather than by carrying a flag someone can forget to read.
    let mut envelope = signed_by_bob();
    envelope.signature = EnvelopeSignature::new([7u8; EnvelopeSignature::LENGTH]);

    let attested =
        VerifiedAuthor::attest(&CheckingVerifier, &envelope).expect("verifier available");

    assert_eq!(attested, None);
}

#[test]
fn a_tampered_payload_invalidates_the_author() {
    let mut envelope = signed_by_bob();
    envelope.payload = b"something else".to_vec();

    assert_eq!(
        VerifiedAuthor::attest(&CheckingVerifier, &envelope).expect("verifier available"),
        None
    );
}

#[test]
fn an_envelope_claiming_another_peer_does_not_attest_that_peer() {
    // The signature was made with bob's key; relabelling the author field is
    // the exact attack invariant 4 exists to defeat.
    let mut envelope = signed_by_bob();
    envelope.author = test_peers::carol();

    assert_eq!(
        VerifiedAuthor::attest(&CheckingVerifier, &envelope).expect("verifier available"),
        None
    );
}

#[test]
fn a_verifier_that_cannot_run_is_never_read_as_valid() {
    // AC6 distinguishes a forged envelope from a broken verifier: the second
    // leaves authenticity *unknown*, which must never collapse into "valid".
    assert_eq!(
        VerifiedAuthor::attest(&UnavailableVerifier, &signed_by_bob()),
        Err(EnvelopeVerifierError::VerifierUnavailable)
    );
}
