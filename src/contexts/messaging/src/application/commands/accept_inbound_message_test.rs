use std::sync::Arc;

use shared_types::{EnvelopeSignature, PayloadKind, ProtocolVersion};

use crate::application::MessagingSettings;
use crate::application::test_context::{
    NOW, TestContext, TestContextBuilder, body, broadcast_from, direct_from, envelope_from,
    sequence,
};
use crate::domain::events::{MessagingEvent, RejectionReason};
use crate::domain::{ConversationId, DurationMillis, MessageId, Millis};
use crate::ports::port_fakes::UnavailableVerifier;
use crate::ports::{
    EnvelopeVerifierPort, MessagePayload, MessagingCommandError, MessagingQueryPort,
};
use crate::test_peers;

/// An instant no clock of this peer's ever reads, so a rule driven by it is
/// unmistakably driven by the author's claim.
const AUTHOR_CLAIM: Millis = Millis::from_millis(5_000_000);

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

fn direct_with_bob() -> ConversationId {
    ConversationId::Direct(test_peers::bob())
}

fn rejections(context: &TestContext) -> Vec<RejectionReason> {
    context
        .events()
        .into_iter()
        .filter_map(|event| match event {
            MessagingEvent::MessageRejected(rejected) => Some(rejected.reason),
            _ => None,
        })
        .collect()
}

#[test]
fn a_signed_message_reaches_the_conversation() {
    let context = alice();

    let verdict = context
        .accept(direct_from(test_peers::bob(), 1, "hello", AUTHOR_CLAIM))
        .expect("the pipeline runs");

    assert!(verdict.is_applied());
    assert_eq!(context.visible_text(direct_with_bob()), vec!["hello"]);
}

#[test]
fn an_invalid_signature_is_rejected_before_the_read_model() {
    // AC6 + invariant 10: content that fails verification never reaches any
    // read model, and the refusal is counted in local diagnostics.
    let context = alice();
    let mut envelope = direct_from(test_peers::bob(), 1, "forged", AUTHOR_CLAIM);
    envelope.signature = EnvelopeSignature::new([0xab; EnvelopeSignature::LENGTH]);

    let verdict = context.accept(envelope).expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::SignatureInvalid)
    );
    assert_eq!(context.history(direct_with_bob()), Vec::new());
    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
    assert_eq!(
        rejections(&context),
        vec![RejectionReason::SignatureInvalid]
    );
}

#[test]
fn an_envelope_relabelled_with_another_peers_identity_is_rejected() {
    // Invariant 4: the author is whoever the signature verifies for, never a
    // field. Bob signs, then claims to be carol.
    let context = alice();
    let mut envelope = direct_from(test_peers::bob(), 1, "not mine", AUTHOR_CLAIM);
    envelope.author = test_peers::carol();

    let verdict = context.accept(envelope).expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::SignatureInvalid)
    );
    assert_eq!(
        context.history(ConversationId::Direct(test_peers::carol())),
        Vec::new()
    );
}

#[test]
fn a_verifier_that_cannot_run_is_an_error_and_not_an_acceptance() {
    // AC6 keeps "forged" and "could not check" apart; treating the second as
    // valid would admit unauthenticated content, and treating it as a rejection
    // would blame a peer for this machine's problem.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_verifier(Arc::new(UnavailableVerifier) as Arc<dyn EnvelopeVerifierPort + Send + Sync>)
        .build();

    let outcome = context.accept(direct_from(test_peers::bob(), 1, "hello", AUTHOR_CLAIM));

    assert!(matches!(outcome, Err(MessagingCommandError::Verifier(_))));
    assert_eq!(context.history(direct_with_bob()), Vec::new());
    assert_eq!(rejections(&context), Vec::new());
}

#[test]
fn a_blocked_authors_message_is_refused_at_the_boundary() {
    // Invariant 11: a blocked peer's envelopes are dropped at the application
    // boundary. Nothing is announced to them and nothing enters the read model.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .blocking([test_peers::bob()])
        .build();

    let verdict = context
        .accept(direct_from(test_peers::bob(), 1, "let me in", AUTHOR_CLAIM))
        .expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::AuthorBlocked)
    );
    assert_eq!(context.history(direct_with_bob()), Vec::new());
    assert_eq!(rejections(&context), vec![RejectionReason::AuthorBlocked]);
}

#[test]
fn blocking_one_peer_does_not_silence_another() {
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .blocking([test_peers::bob()])
        .build();

    context
        .accept(broadcast_from(
            test_peers::bob(),
            1,
            "blocked",
            AUTHOR_CLAIM,
        ))
        .expect("refusal");
    context
        .accept(broadcast_from(
            test_peers::carol(),
            1,
            "heard",
            AUTHOR_CLAIM,
        ))
        .expect("accepted");

    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["heard"]
    );
}

#[test]
fn the_block_check_cannot_be_bypassed_by_claiming_another_identity() {
    // The signature check runs *before* the block list precisely so that
    // putting an unblocked peer's `PeerId` in the author field cannot smuggle a
    // blocked peer's content past it: the envelope simply stops verifying.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .blocking([test_peers::bob()])
        .build();
    let mut envelope = direct_from(test_peers::bob(), 1, "sneaking in", AUTHOR_CLAIM);
    envelope.author = test_peers::carol();

    let verdict = context.accept(envelope).expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::SignatureInvalid)
    );
    assert_eq!(
        context.history(ConversationId::Direct(test_peers::carol())),
        Vec::new()
    );
}

#[test]
fn an_unsupported_major_version_is_rejected_with_a_stated_reason() {
    // AC14: a different major version is a wire format this build cannot read.
    let context = alice();
    let envelope = crate::application::test_context::envelope_versioned(
        test_peers::bob(),
        PayloadKind::DirectMessage,
        MessagePayload::new(sequence(1), AUTHOR_CLAIM, body("from the future")),
        ProtocolVersion::new(2, 0),
    );

    let verdict = context.accept(envelope).expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::UnsupportedProtocolVersion)
    );
    assert_eq!(context.history(direct_with_bob()), Vec::new());
}

#[test]
fn a_higher_minor_version_is_tolerated_and_read() {
    // The other half of AC14: same major, newer minor — the sender is ahead,
    // and peers upgrade independently, so this must still be read.
    let context = alice();
    let envelope = crate::application::test_context::envelope_versioned(
        test_peers::bob(),
        PayloadKind::DirectMessage,
        MessagePayload::new(sequence(1), AUTHOR_CLAIM, body("newer minor")),
        ProtocolVersion::new(
            ProtocolVersion::CURRENT.major,
            ProtocolVersion::CURRENT.minor + 3,
        ),
    );

    let verdict = context.accept(envelope).expect("tolerated");

    assert!(verdict.is_applied());
    assert_eq!(context.visible_text(direct_with_bob()), vec!["newer minor"]);
}

#[test]
fn an_unknown_payload_kind_is_tolerated_rather_than_refused() {
    // S2/AC14: an unassigned kind is a newer peer speaking, not an attack. It
    // is counted and ignored, never rejected.
    let context = alice();
    let envelope = envelope_from(
        test_peers::bob(),
        PayloadKind::Unknown(4_242),
        MessagePayload::new(sequence(1), AUTHOR_CLAIM, body("a kind from later")),
    );

    let verdict = context.accept(envelope).expect("tolerated");

    assert!(!verdict.is_refused());
    assert_eq!(verdict.rejection_reason(), None);
    assert_eq!(rejections(&context), Vec::new());
    assert_eq!(context.history(direct_with_bob()), Vec::new());
}

#[test]
fn a_heartbeat_is_not_a_message_here() {
    let context = alice();
    let envelope = envelope_from(
        test_peers::bob(),
        PayloadKind::Heartbeat,
        MessagePayload::new(sequence(1), AUTHOR_CLAIM, body("alive")),
    );

    assert!(!context.accept(envelope).expect("tolerated").is_refused());
    assert_eq!(context.history(direct_with_bob()), Vec::new());
}

#[test]
fn an_unreadable_payload_is_refused() {
    // The payload is replaced *before* signing, so the refusal is
    // unambiguously about the payload and not about the signature.
    let context = alice();
    let mut envelope = direct_from(test_peers::bob(), 1, "placeholder", AUTHOR_CLAIM);
    envelope.payload = b"not a payload".to_vec();
    envelope.signature =
        crate::ports::port_fakes::fake_signature(&test_peers::bob(), &envelope.signable_bytes());

    let verdict = context.accept(envelope).expect("a refusal is not an error");

    assert_eq!(
        verdict.rejection_reason(),
        Some(RejectionReason::MalformedPayload)
    );
}

#[test]
fn a_redelivered_message_is_applied_exactly_once() {
    // AC7: exactly-once application over at-least-once delivery. Redelivery by
    // any path changes nothing user-visible.
    let context = alice();
    let envelope = broadcast_from(test_peers::bob(), 1, "said once", AUTHOR_CLAIM);

    let first = context.accept(envelope.clone()).expect("first arrival");
    let second = context.accept(envelope.clone()).expect("redelivery");
    let third = context.accept(envelope).expect("redelivery again");

    assert!(first.is_applied());
    assert!(second.is_duplicate());
    assert!(third.is_duplicate());
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["said once"]
    );
    assert_eq!(context.mirrored(ConversationId::Broadcast).len(), 1);
}

#[test]
fn messages_that_arrive_out_of_order_display_in_send_order() {
    // AC8: one author's messages display in that author's send order whatever
    // order they arrive in.
    let context = alice();
    let sent: Vec<_> = (1..=4)
        .map(|seq| {
            broadcast_from(
                test_peers::bob(),
                seq,
                &format!("message {seq}"),
                AUTHOR_CLAIM,
            )
        })
        .collect();

    for index in [3usize, 0, 2, 1] {
        context
            .accept(sent[index].clone())
            .expect("every arrival is judged");
    }

    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["message 1", "message 2", "message 3", "message 4"]
    );
}

#[test]
fn a_message_waiting_behind_a_gap_is_visible_to_nothing() {
    // Invariant 5: a buffered arrival is not part of the conversation, so no
    // read may show it — showing it would be showing an author out of order.
    let context = alice();

    let verdict = context
        .accept(broadcast_from(test_peers::bob(), 7, "early", AUTHOR_CLAIM))
        .expect("buffered");

    assert!(verdict.is_buffered());
    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
    assert_eq!(context.mirrored(ConversationId::Broadcast).len(), 0);
    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
}

#[test]
fn the_arrival_instant_comes_from_this_peers_clock_and_not_the_authors_claim() {
    // Rule R ages a gap from the local arrival. This author claims to have
    // sent far in the future: if the claim aged the gap, the elapsed span
    // would saturate to zero and the gap would never close.
    let settings = MessagingSettings::for_local_peer(test_peers::alice())
        .with_gap_tolerance(DurationMillis::from_millis(2_000));
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_settings(settings)
        .build();

    // 1 first, so the sweep abandons a run that genuinely was in flight rather
    // than merely establishing where bob's stream starts here (D10).
    context
        .accept(broadcast_from(test_peers::bob(), 1, "first", AUTHOR_CLAIM))
        .expect("applied");
    context
        .accept(broadcast_from(
            test_peers::bob(),
            5,
            "from the future",
            AUTHOR_CLAIM,
        ))
        .expect("buffered");
    context.clock.advance(2_000);

    let closed = {
        use crate::ports::InboundEnvelopePort;
        context.context.inbound().close_aged_gaps().expect("sweep")
    };

    assert_eq!(closed.len(), 1, "the local arrival aged, so the gap closed");
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["first", "from the future"]
    );
}

#[test]
fn an_author_backdating_its_claim_cannot_force_a_gap_shut_early() {
    // The other direction. The claim is ancient; the local arrival is recent.
    // If the claim aged the gap it would already be far past tolerance.
    let settings = MessagingSettings::for_local_peer(test_peers::alice())
        .with_gap_tolerance(DurationMillis::from_millis(2_000));
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_settings(settings)
        .build();

    context
        .accept(broadcast_from(
            test_peers::bob(),
            5,
            "backdated",
            Millis::ZERO,
        ))
        .expect("buffered");
    // The clock reads NOW (1_000_000 ms), a million milliseconds past the
    // claim, but only 500 ms past the arrival.
    context.clock.advance(500);

    let closed = {
        use crate::ports::InboundEnvelopePort;
        context.context.inbound().close_aged_gaps().expect("sweep")
    };

    assert_eq!(closed, Vec::new(), "only 500 ms of local time have passed");
    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
    assert_eq!(NOW.as_millis(), 1_000_000);
}

#[test]
fn a_message_this_peer_authored_cannot_arrive_from_the_network() {
    // A replay of this peer's own traffic. The aggregate refuses it as a caller
    // mistake rather than as a network condition, because it is one.
    let context = alice();
    let envelope = broadcast_from(test_peers::alice(), 1, "my own words", AUTHOR_CLAIM);

    let outcome = context.accept(envelope);

    assert!(matches!(
        outcome,
        Err(MessagingCommandError::Conversation(_))
    ));
}

#[test]
fn an_applied_message_is_mirrored_into_the_log_and_announced() {
    let context = alice();

    context
        .accept(direct_from(test_peers::bob(), 1, "mirrored", AUTHOR_CLAIM))
        .expect("applied");

    let logged = context.mirrored(direct_with_bob());
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].body().as_str(), "mirrored");

    let id = MessageId::new(test_peers::bob(), direct_with_bob(), sequence(1));
    assert!(context.events().iter().any(|event| matches!(
        event,
        MessagingEvent::MessageReceived(received) if received.id == id
    )));
}
