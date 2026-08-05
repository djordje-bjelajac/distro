use shared_types::{EnvelopeSignature, Fingerprint, PayloadKind, ProtocolVersion};

use crate::domain::events::{DisplayNameChanged, LocalIdentityInitialized};
use crate::domain::{DisplayName, LocalIdentity};
use crate::test_peers;

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

fn alice() -> (LocalIdentity, LocalIdentityInitialized) {
    LocalIdentity::initialize(test_peers::alice(), name("Ada"))
}

#[test]
fn initializing_yields_the_identity_and_a_past_tense_event() {
    let (identity, event) = alice();

    assert_eq!(identity.peer_id(), test_peers::alice());
    assert_eq!(identity.display_name(), &name("Ada"));
    assert_eq!(
        event,
        LocalIdentityInitialized {
            peer: test_peers::alice(),
            display_name: name("Ada"),
        }
    );
}

#[test]
fn exposes_the_fingerprint_used_for_out_of_band_verification() {
    let (identity, _) = alice();

    assert_eq!(
        identity.fingerprint(),
        Fingerprint::of(&test_peers::alice())
    );
}

#[test]
fn changing_the_display_name_reports_both_sides_of_the_change() {
    let (mut identity, _) = alice();

    let event = identity.change_display_name(name("Grace"));

    assert_eq!(
        event,
        Some(DisplayNameChanged {
            peer: test_peers::alice(),
            previous: name("Ada"),
            current: name("Grace"),
        })
    );
    assert_eq!(identity.display_name(), &name("Grace"));
}

#[test]
fn setting_the_name_it_already_has_changes_nothing_and_emits_nothing() {
    let (mut identity, _) = alice();

    let event = identity.change_display_name(name("  Ada  "));

    assert_eq!(event, None, "no change occurred, so no change is reported");
    assert_eq!(identity.display_name(), &name("Ada"));
}

#[test]
fn the_display_name_never_participates_in_identity() {
    let (ada, _) = LocalIdentity::initialize(test_peers::alice(), name("Ada"));
    let (grace, _) = LocalIdentity::initialize(test_peers::alice(), name("Grace"));

    assert_eq!(ada.peer_id(), grace.peer_id());
    assert_eq!(ada.fingerprint(), grace.fingerprint());
}

#[test]
fn drafts_envelopes_authored_by_itself_at_this_builds_protocol_version() {
    let (identity, _) = alice();

    let draft = identity.draft_envelope(PayloadKind::DirectMessage, b"hi".to_vec());

    assert_eq!(
        draft.author(),
        test_peers::alice(),
        "the author is the local peer, never a payload field (invariant 4)"
    );
    assert_eq!(draft.version(), ProtocolVersion::CURRENT);
    assert_eq!(draft.kind(), PayloadKind::DirectMessage);
    assert_eq!(draft.payload(), b"hi");
}

#[test]
fn a_draft_carries_the_exact_bytes_the_signed_envelope_is_verified_over() {
    let (identity, _) = alice();
    let draft = identity.draft_envelope(PayloadKind::BroadcastMessage, b"news".to_vec());
    let signable = draft.signable_bytes();

    let signed = draft.into_signed(EnvelopeSignature::new([9u8; EnvelopeSignature::LENGTH]));

    assert_eq!(signed.signable_bytes(), signable);
    assert_eq!(signed.author, identity.peer_id());
}

#[test]
fn drafts_from_different_identities_differ_in_their_signable_bytes() {
    let (ada, _) = LocalIdentity::initialize(test_peers::alice(), name("Ada"));
    let (bob, _) = LocalIdentity::initialize(test_peers::bob(), name("Bob"));

    let from_ada = ada.draft_envelope(PayloadKind::DirectMessage, b"hi".to_vec());
    let from_bob = bob.draft_envelope(PayloadKind::DirectMessage, b"hi".to_vec());

    assert_ne!(from_ada.signable_bytes(), from_bob.signable_bytes());
}
