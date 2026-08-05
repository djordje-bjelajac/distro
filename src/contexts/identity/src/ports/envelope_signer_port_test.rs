use shared_types::PayloadKind;

use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::port_fakes::{FailingSigner, RecordingSigner, fake_signature};
use crate::ports::{EnvelopeSignerError, EnvelopeSignerPort};
use crate::test_peers;

fn alice() -> LocalIdentity {
    let (identity, _) = LocalIdentity::initialize(
        test_peers::alice(),
        DisplayName::new("Ada").expect("valid fixture"),
    );
    identity
}

#[test]
fn signs_exactly_the_drafts_signable_bytes_and_nothing_else() {
    let identity = alice();
    let draft = identity.draft_envelope(PayloadKind::DirectMessage, b"hi".to_vec());
    let expected_input = draft.signable_bytes();
    let signer = RecordingSigner::holding_key_of(test_peers::alice());

    let signature = signer.sign(&draft).expect("the fake signer succeeds");

    assert_eq!(
        signer.signed_inputs(),
        vec![expected_input.clone()],
        "the port received the envelope's signable bytes verbatim"
    );
    assert_eq!(
        signature,
        fake_signature(&test_peers::alice(), &expected_input)
    );
}

#[test]
fn sealing_completes_the_draft_with_the_signature_over_those_same_bytes() {
    let identity = alice();
    let draft = identity.draft_envelope(PayloadKind::BroadcastMessage, b"news".to_vec());
    let expected_input = draft.signable_bytes();
    let signer = RecordingSigner::holding_key_of(test_peers::alice());

    let envelope = signer.seal(draft).expect("the fake signer succeeds");

    assert_eq!(signer.signed_inputs(), vec![expected_input.clone()]);
    assert_eq!(
        envelope.signable_bytes(),
        expected_input,
        "what was signed is what any receiver will verify"
    );
    assert_eq!(
        envelope.signature,
        fake_signature(&test_peers::alice(), &expected_input)
    );
    assert_eq!(envelope.author, test_peers::alice());
    assert_eq!(envelope.kind, PayloadKind::BroadcastMessage);
    assert_eq!(envelope.payload, b"news".to_vec());
}

#[test]
fn sealing_works_through_a_trait_object() {
    let identity = alice();
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let port: &dyn EnvelopeSignerPort = &signer;

    let envelope = port
        .seal(identity.draft_envelope(PayloadKind::Heartbeat, Vec::new()))
        .expect("the fake signer succeeds");

    assert_eq!(envelope.kind, PayloadKind::Heartbeat);
    assert_eq!(signer.signed_inputs().len(), 1);
}

#[test]
fn a_signer_failure_surfaces_as_a_typed_error_from_both_methods() {
    let identity = alice();
    let signer = FailingSigner(EnvelopeSignerError::KeyUnavailable);
    let port: &dyn EnvelopeSignerPort = &signer;

    let draft = identity.draft_envelope(PayloadKind::DirectMessage, b"hi".to_vec());
    assert_eq!(
        port.sign(&draft),
        Err(EnvelopeSignerError::KeyUnavailable),
        "no panic, no forged signature"
    );
    assert_eq!(
        port.seal(draft),
        Err(EnvelopeSignerError::KeyUnavailable),
        "a failed signature must not yield an envelope"
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            EnvelopeSignerError::KeyUnavailable,
            "local signing key is unavailable",
        ),
        (
            EnvelopeSignerError::SigningFailed,
            "envelope could not be signed",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
