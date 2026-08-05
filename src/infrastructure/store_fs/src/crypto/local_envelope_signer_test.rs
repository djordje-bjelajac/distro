use identity::domain::{DisplayName, LocalIdentity};
use identity::ports::{
    EnvelopeSignerPort as IdentitySignerPort, EnvelopeVerifierPort as IdentityVerifierPort,
    SignatureVerdict as IdentityVerdict,
};
use messaging::ports::{
    EnvelopeSignerPort as MessagingSignerPort, EnvelopeVerifierPort as MessagingVerifierPort,
    SignatureVerdict as MessagingVerdict, UnsignedEnvelope as MessagingDraft,
};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::crypto::LocalEnvelopeSigner;
use crate::format::hex_bytes;
use crate::stores::FileIdentityKeyStore;
use crate::test_dir::TestDir;
use crate::test_peers::{ALICE_SECRET_KEY, alice, bob};

/// A signer over a freshly created identity in its own directory.
fn signer(label: &str) -> (TestDir, LocalEnvelopeSigner) {
    let dir = TestDir::new(label);
    let signer = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME))
        .load_or_create_signer()
        .expect("a fresh directory must yield a signer");

    (dir, signer)
}

/// A signer over the RFC 8032 §7.1 TEST 1 key, so its `PeerId` is [`alice`].
fn alice_signer(label: &str) -> (TestDir, LocalEnvelopeSigner) {
    let dir = TestDir::new(label);
    let path = dir.file(FileIdentityKeyStore::FILE_NAME);
    std::fs::write(
        &path,
        format!(
            "distro-identity-key 1\ned25519-seed {}\n",
            hex_bytes::encode(&ALICE_SECRET_KEY)
        ),
    )
    .expect("the plant must land");

    let signer = FileIdentityKeyStore::at(&path)
        .load_or_create_signer()
        .expect("the planted key must load");

    (dir, signer)
}

fn identity_draft(peer: PeerId) -> identity::domain::UnsignedEnvelope {
    // `identity`'s draft constructor is crate-private on purpose: every draft
    // must come from the local aggregate, so this is the only way to make one.
    let (identity, _) = LocalIdentity::initialize(
        peer,
        DisplayName::new("tester").expect("a valid fixture name"),
    );

    identity.draft_envelope(PayloadKind::Heartbeat, b"identity payload".to_vec())
}

fn messaging_draft(peer: PeerId) -> MessagingDraft {
    MessagingDraft::draft(
        peer,
        ProtocolVersion::CURRENT,
        PayloadKind::BroadcastMessage,
        b"messaging payload".to_vec(),
    )
}

#[test]
fn identitys_signature_verifies_through_identitys_verifier() {
    let (_dir, signer) = signer("signer-identity-round-trip");

    let envelope = IdentitySignerPort::seal(&signer, identity_draft(signer.peer()))
        .expect("the local peer's own draft must be signable");

    assert_eq!(
        IdentityVerifierPort::verify(&signer, &envelope),
        Ok(IdentityVerdict::Valid)
    );
}

#[test]
fn messagings_signature_verifies_through_messagings_verifier() {
    let (_dir, signer) = signer("signer-messaging-round-trip");

    let envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the local peer's own draft must be signable");

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Valid)
    );
}

#[test]
fn each_contexts_signature_verifies_through_the_other_contexts_verifier() {
    let (_dir, signer) = signer("signer-cross-context");

    let from_identity = IdentitySignerPort::seal(&signer, identity_draft(signer.peer()))
        .expect("the draft must be signable");
    let from_messaging = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    // One key behind four ports (canvas §4): if the two contexts were ever
    // wired to different keys, exactly this would break — and only here.
    assert_eq!(
        MessagingVerifierPort::verify(&signer, &from_identity),
        Ok(MessagingVerdict::Valid)
    );
    assert_eq!(
        IdentityVerifierPort::verify(&signer, &from_messaging),
        Ok(IdentityVerdict::Valid)
    );
}

#[test]
fn the_signer_speaks_for_the_peer_the_key_store_reports() {
    let dir = TestDir::new("signer-same-identity");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    let peer = identity::ports::IdentityKeyStorePort::load_or_create_local_peer(&store)
        .expect("an identity must be created");
    let signer = store
        .load_or_create_signer()
        .expect("the same key must load");

    assert_eq!(signer.peer(), peer);
}

#[test]
fn the_key_store_reports_the_peer_a_signer_created_first() {
    let dir = TestDir::new("signer-creates-first");
    let store = FileIdentityKeyStore::at(dir.file(FileIdentityKeyStore::FILE_NAME));

    // Either entry point may be the one that creates the file; both must then
    // agree, or the peer would sign as an identity it does not report.
    let signer = store
        .load_or_create_signer()
        .expect("a fresh directory must yield a signer");

    assert_eq!(
        identity::ports::IdentityKeyStorePort::load_or_create_local_peer(&store),
        Ok(signer.peer())
    );
}

#[test]
fn a_signer_loaded_after_a_restart_still_signs_as_the_same_peer() {
    let dir = TestDir::new("signer-restart");
    let path = dir.file(FileIdentityKeyStore::FILE_NAME);

    let before = FileIdentityKeyStore::at(&path)
        .load_or_create_signer()
        .expect("first launch must yield a signer");
    let peer_before = before.peer();

    // The next launch: a new store, a new signer, the same file.
    let after = FileIdentityKeyStore::at(&path)
        .load_or_create_signer()
        .expect("a restart must load the signer");

    let envelope = MessagingSignerPort::seal(&after, messaging_draft(peer_before))
        .expect("the restored key must sign for the identity it restored");

    assert_eq!(after.peer(), peer_before, "AC9");
    // Verified through the *pre-restart* signer's verifier: peers that were
    // already online keep accepting this peer's envelopes across its restart.
    assert_eq!(
        MessagingVerifierPort::verify(&before, &envelope),
        Ok(MessagingVerdict::Valid)
    );
    assert_eq!(
        IdentityVerifierPort::verify(&before, &envelope),
        Ok(IdentityVerdict::Valid)
    );
}

#[test]
fn a_signature_from_one_key_is_invalid_under_another_peers_identity() {
    let (_alice_dir, alice_signer) = alice_signer("signer-foreign-alice");
    let (_bob_dir, other) = signer("signer-foreign-other");

    let mut envelope = MessagingSignerPort::seal(&alice_signer, messaging_draft(alice()))
        .expect("the draft must be signable");

    // The signature is real, the author is somebody else: this is the forgery
    // invariant 4 exists to catch, and it must be caught by arithmetic rather
    // than by any lookup.
    envelope.author = other.peer();

    assert_eq!(
        MessagingVerifierPort::verify(&other, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
    assert_eq!(
        IdentityVerifierPort::verify(&alice_signer, &envelope),
        Ok(IdentityVerdict::Invalid)
    );
}

#[test]
fn a_signature_lifted_onto_another_peers_envelope_is_invalid() {
    let (_dir, signer) = signer("signer-lifted-signature");
    let signed = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    let forged = Envelope {
        author: bob(),
        signature: signed.signature,
        ..signed
    };

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &forged),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn tampering_with_the_version_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-version");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.version = ProtocolVersion::new(envelope.version.major + 1, envelope.version.minor);

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn tampering_with_the_minor_version_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-minor");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.version = ProtocolVersion::new(envelope.version.major, envelope.version.minor + 1);

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn tampering_with_the_kind_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-kind");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.kind = PayloadKind::DirectMessage;

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn tampering_with_the_author_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-author");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.author = alice();

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn tampering_with_the_payload_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-payload");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.payload.push(b'!');

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn truncating_the_payload_invalidates_the_signature() {
    let (_dir, signer) = signer("signer-tamper-truncate");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.payload.pop();

    // The length prefix in the signable layout is what makes this fail for the
    // right reason rather than by luck.
    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn a_replaced_signature_is_invalid_rather_than_a_panic() {
    let (_dir, signer) = signer("signer-garbage-signature");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    envelope.signature = EnvelopeSignature::new([0xffu8; EnvelopeSignature::LENGTH]);

    // Hostile input is the normal case on an open network: a verdict, never an
    // `Err`, and never a panic (AC6).
    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
    assert_eq!(
        IdentityVerifierPort::verify(&signer, &envelope),
        Ok(IdentityVerdict::Invalid)
    );
}

#[test]
fn an_all_zero_signature_is_invalid_rather_than_a_panic() {
    let (_dir, signer) = signer("signer-zero-signature");
    let mut envelope = MessagingSignerPort::seal(&signer, messaging_draft(signer.peer()))
        .expect("the draft must be signable");

    // The placeholder an unsigned draft carries: an unsigned envelope must
    // never read as a valid one.
    envelope.signature = EnvelopeSignature::new([0u8; EnvelopeSignature::LENGTH]);

    assert_eq!(
        MessagingVerifierPort::verify(&signer, &envelope),
        Ok(MessagingVerdict::Invalid)
    );
}

#[test]
fn messaging_refuses_a_draft_naming_a_foreign_author() {
    let (_dir, signer) = signer("signer-messaging-foreign-author");

    // Signing it anyway would emit an envelope asserting an identity this peer
    // cannot back — no verifier anywhere would accept it.
    assert_eq!(
        MessagingSignerPort::sign(&signer, &messaging_draft(bob())),
        Err(messaging::ports::EnvelopeSignerError::AuthorMismatch)
    );
}

#[test]
fn messaging_refuses_to_seal_a_draft_naming_a_foreign_author() {
    let (_dir, signer) = signer("signer-messaging-foreign-seal");

    assert_eq!(
        MessagingSignerPort::seal(&signer, messaging_draft(bob())).err(),
        Some(messaging::ports::EnvelopeSignerError::AuthorMismatch)
    );
}

#[test]
fn identity_refuses_a_draft_naming_a_foreign_author() {
    let (_dir, signer) = signer("signer-identity-foreign-author");

    // `identity`'s error set has no `AuthorMismatch`; `KeyUnavailable` is
    // literally true — this signer holds no key for that author — and it is the
    // variant `infra-sim-net` returns here too.
    assert_eq!(
        IdentitySignerPort::sign(&signer, &identity_draft(bob())),
        Err(identity::ports::EnvelopeSignerError::KeyUnavailable)
    );
}

#[test]
fn identity_refuses_to_seal_a_draft_naming_a_foreign_author() {
    let (_dir, signer) = signer("signer-identity-foreign-seal");

    assert_eq!(
        IdentitySignerPort::seal(&signer, identity_draft(bob())).err(),
        Some(identity::ports::EnvelopeSignerError::KeyUnavailable)
    );
}

#[test]
fn signing_is_deterministic_for_one_draft() {
    let (_dir, signer) = signer("signer-deterministic");
    let draft = messaging_draft(signer.peer());

    // Ed25519 signatures are deterministic; a signer that produced two
    // different signatures for one draft would be reaching for randomness it
    // has no business having (S5).
    assert_eq!(
        MessagingSignerPort::sign(&signer, &draft),
        MessagingSignerPort::sign(&signer, &draft)
    );
}

#[test]
fn the_signature_covers_the_envelopes_own_signable_bytes() {
    let (_dir, signer) = alice_signer("signer-signable-bytes");
    let draft = messaging_draft(alice());
    let draft_bytes = draft.signable_bytes();

    let envelope = MessagingSignerPort::seal(&signer, draft).expect("the draft must be signable");

    // The draft and the envelope it becomes must present identical signing
    // input, or a signature made here would not verify at any peer (S2).
    assert_eq!(envelope.signable_bytes(), draft_bytes);
}

#[test]
fn the_debug_output_holds_no_key_material() {
    let (_dir, signer) = alice_signer("signer-debug");
    let rendered = format!("{signer:?}");
    let secret_hex = hex_bytes::encode(&ALICE_SECRET_KEY);

    assert!(rendered.contains("LocalEnvelopeSigner"));
    assert!(
        !rendered.contains(&secret_hex),
        "the whole secret must not appear"
    );

    // Not just the whole key: no recognisable fragment of it either, in either
    // rendering a leak would plausibly take. Hex catches a `Debug` that
    // formatted the key as a string; the decimal list catches the likelier
    // accident — a derived impl, or one that printed `SigningKey::to_bytes`,
    // which renders a `[u8; 32]` as decimal numbers and would sail past a
    // hex-only check.
    let lowered = rendered.to_lowercase();

    for window in secret_hex.as_bytes().windows(8) {
        let fragment = std::str::from_utf8(window).expect("hex is ASCII");

        assert!(
            !lowered.contains(fragment),
            "a hex fragment of the secret key appeared in Debug output: {fragment}"
        );
    }

    let decimal: Vec<String> = ALICE_SECRET_KEY.iter().map(u8::to_string).collect();

    for window in decimal.windows(5) {
        let fragment = window.join(", ");

        assert!(
            !rendered.contains(&fragment),
            "a decimal fragment of the secret key appeared in Debug output: {fragment}"
        );
    }

    // And nothing was lost: the public fingerprint is what a signer is allowed
    // to say about itself.
    assert!(rendered.contains(&shared_types::Fingerprint::of(&alice()).to_string()));
}
