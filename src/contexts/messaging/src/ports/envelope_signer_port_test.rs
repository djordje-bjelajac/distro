use shared_types::{Envelope, PayloadKind, ProtocolVersion};

use crate::ports::port_fakes::{FailingSigner, RecordingSigner, fake_signature};
use crate::ports::{EnvelopeSignerError, EnvelopeSignerPort, UnsignedEnvelope};
use crate::test_peers;

fn draft() -> UnsignedEnvelope {
    UnsignedEnvelope::draft(
        test_peers::alice(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"a message payload".to_vec(),
    )
}

#[test]
fn the_port_is_object_safe_so_one_signer_can_be_shared() {
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let port: &dyn EnvelopeSignerPort = &signer;

    assert!(port.sign(&draft()).is_ok());
}

#[test]
fn what_gets_signed_is_exactly_the_envelopes_signable_bytes() {
    // The whole contract: whatever codec carries the envelope, the signing
    // input is the layout pinned in `shared_types`, so a peer running a
    // different codec still verifies this signature (S2, invariant 4).
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let draft = draft();
    let expected = draft.signable_bytes();

    signer.sign(&draft).expect("the local key is available");

    assert_eq!(signer.signed_inputs(), [expected]);
}

#[test]
fn a_draft_and_the_envelope_it_becomes_have_identical_signing_input() {
    // The signature field is not covered by the layout, so completing a draft
    // cannot change what was signed.
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let draft = draft();
    let before = draft.signable_bytes();

    let envelope = signer.seal(draft).expect("the local key is available");

    assert_eq!(envelope.signable_bytes(), before);
}

#[test]
fn sealing_produces_a_wire_ready_envelope_carrying_the_drafted_fields() {
    let signer = RecordingSigner::holding_key_of(test_peers::alice());

    let envelope = signer.seal(draft()).expect("the local key is available");

    assert_eq!(
        envelope,
        Envelope {
            version: ProtocolVersion::CURRENT,
            kind: PayloadKind::DirectMessage,
            author: test_peers::alice(),
            payload: b"a message payload".to_vec(),
            signature: fake_signature(&test_peers::alice(), &envelope.signable_bytes()),
        }
    );
}

#[test]
fn signing_a_draft_authored_by_another_peer_is_refused() {
    // A signer holds one key. Producing a signature under someone else's
    // identity is not something a wrong key does badly — it is something no
    // implementation may attempt.
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let foreign = UnsignedEnvelope::draft(
        test_peers::bob(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"not mine to sign".to_vec(),
    );

    assert_eq!(
        signer.sign(&foreign),
        Err(EnvelopeSignerError::AuthorMismatch)
    );
    assert!(signer.signed_inputs().is_empty());
}

#[test]
fn an_unavailable_key_is_a_typed_error_not_an_unsigned_envelope() {
    let signer = FailingSigner(EnvelopeSignerError::KeyUnavailable);

    assert_eq!(
        signer.seal(draft()),
        Err(EnvelopeSignerError::KeyUnavailable)
    );
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        EnvelopeSignerError::KeyUnavailable.to_string(),
        "local signing key is unavailable"
    );
    assert_eq!(
        EnvelopeSignerError::SigningFailed.to_string(),
        "envelope could not be signed"
    );
    assert_eq!(
        EnvelopeSignerError::AuthorMismatch.to_string(),
        "the draft names an author this signer holds no key for"
    );
}

#[test]
fn a_draft_exposes_what_it_will_become_without_a_signature_in_sight() {
    let draft = draft();

    assert_eq!(draft.author(), test_peers::alice());
    assert_eq!(draft.version(), ProtocolVersion::CURRENT);
    assert_eq!(draft.kind(), PayloadKind::DirectMessage);
    assert_eq!(draft.payload(), b"a message payload");
}
