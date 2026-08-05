use std::sync::Arc;

use messaging::ports::{EnvelopeSignerPort as MessagingSigner, UnsignedEnvelope};
use shared_types::{PayloadKind, ProtocolVersion};

use crate::crypto::{SimKeypair, SimSigner, SimVerifier};

const SEED: u64 = 0x51_6E_ED;

fn draft_authored_by(author: shared_types::PeerId) -> UnsignedEnvelope {
    UnsignedEnvelope::draft(
        author,
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        b"payload".to_vec(),
    )
}

#[test]
fn a_sealed_envelope_verifies_against_its_author() {
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let signer = SimSigner::new(Arc::clone(&keypair));

    let envelope = signer
        .seal(draft_authored_by(keypair.peer()))
        .expect("the signer holds this author's key");

    assert!(SimVerifier::verifies(&envelope));
}

#[test]
fn a_draft_naming_another_author_is_refused_as_an_author_mismatch() {
    // The contract messaging's port states outright: signing this would produce
    // an envelope asserting an identity this peer cannot back.
    let alice = Arc::new(SimKeypair::derived(SEED, "alice"));
    let bob = SimKeypair::derived(SEED, "bob");
    let signer = SimSigner::new(alice);

    let refusal = MessagingSigner::sign(&signer, &draft_authored_by(bob.peer()));

    assert_eq!(
        refusal,
        Err(messaging::ports::EnvelopeSignerError::AuthorMismatch)
    );
}

#[test]
fn identity_reports_a_foreign_author_as_a_key_it_does_not_hold() {
    // `identity`'s port has no AuthorMismatch variant, and KeyUnavailable is
    // the literal truth: this signer holds no key for that author.
    use identity::ports::EnvelopeSignerPort as IdentitySigner;

    let alice = Arc::new(SimKeypair::derived(SEED, "alice"));
    let signer = SimSigner::new(alice);

    let bob = SimKeypair::derived(SEED, "bob").peer();
    let (bobs_identity, _) = identity::domain::LocalIdentity::initialize(
        bob,
        identity::domain::DisplayName::derived_from(&bob),
    );

    let refusal = IdentitySigner::sign(
        &signer,
        &bobs_identity.draft_envelope(PayloadKind::DirectMessage, b"payload".to_vec()),
    );

    assert_eq!(
        refusal,
        Err(identity::ports::EnvelopeSignerError::KeyUnavailable)
    );
}

#[test]
fn both_context_ports_are_backed_by_one_key() {
    // Canvas §4: the composition root wires both signer ports to the one
    // underlying signer. Two impls, one keypair — the wiring mistake is not
    // expressible.
    let keypair = Arc::new(SimKeypair::derived(SEED, "alice"));
    let signer = SimSigner::new(Arc::clone(&keypair));

    assert_eq!(signer.peer(), keypair.peer());

    let messaging_signature = MessagingSigner::sign(&signer, &draft_authored_by(keypair.peer()))
        .expect("the signer holds this author's key");

    assert_eq!(
        messaging_signature,
        keypair.sign_bytes(&draft_authored_by(keypair.peer()).signable_bytes())
    );
}
