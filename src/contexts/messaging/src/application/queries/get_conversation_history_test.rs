use crate::application::test_context::{TestContext, TestContextBuilder, broadcast_from};
use crate::domain::{ConversationId, DeliveryState, Millis};
use crate::ports::MessagingQueryPort;
use crate::test_peers;

const CLAIMED_AT: Millis = Millis::from_millis(7);

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

#[test]
fn a_conversation_nobody_has_spoken_in_reads_as_empty() {
    let context = alice();

    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
    assert_eq!(
        context.history(ConversationId::Direct(test_peers::bob())),
        Vec::new()
    );
}

#[test]
fn reading_a_conversation_never_brings_it_into_existence() {
    // The registry opens a conversation on first *touch*; a read must not be a
    // touch, or rendering an empty pane would change what `conversations`
    // reports.
    let context = alice();

    for _ in 0..5 {
        let _ = context.history(ConversationId::Direct(test_peers::dave()));
        let _ = context
            .context
            .queries()
            .delivery_state(crate::domain::MessageId::new(
                test_peers::dave(),
                ConversationId::Direct(test_peers::dave()),
                crate::application::test_context::sequence(1),
            ));
    }

    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
}

#[test]
fn a_buffered_message_is_not_in_the_history() {
    // Invariant 5: a held message is not part of the conversation, and showing
    // it would show its author out of that author's own send order (AC8).
    let context = alice();

    context
        .accept(broadcast_from(
            test_peers::bob(),
            4,
            "arrived early",
            CLAIMED_AT,
        ))
        .expect("buffered");

    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
}

#[test]
fn only_the_applied_prefix_of_a_partly_buffered_run_is_visible() {
    let context = alice();

    context
        .accept(broadcast_from(test_peers::bob(), 1, "first", CLAIMED_AT))
        .expect("applied");
    context
        .accept(broadcast_from(test_peers::bob(), 4, "fourth", CLAIMED_AT))
        .expect("buffered");

    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["first"]
    );
}

#[test]
fn each_authors_messages_hold_their_own_send_order() {
    // AC8. There is no order *across* authors and none is invented — the
    // handler groups by `PeerId`, which is deterministic (AC13).
    let context = alice();

    context
        .accept(broadcast_from(
            test_peers::carol(),
            2,
            "carol two",
            CLAIMED_AT,
        ))
        .expect("buffered");
    context
        .accept(broadcast_from(test_peers::bob(), 1, "bob one", CLAIMED_AT))
        .expect("applied");
    context
        .accept(broadcast_from(
            test_peers::carol(),
            1,
            "carol one",
            CLAIMED_AT,
        ))
        .expect("applied");
    context
        .accept(broadcast_from(test_peers::bob(), 2, "bob two", CLAIMED_AT))
        .expect("applied");

    let visible = context.visible_text(ConversationId::Broadcast);
    let bob_at = |text: &str| {
        visible
            .iter()
            .position(|entry| entry == text)
            .expect("present")
    };

    assert!(bob_at("bob one") < bob_at("bob two"));
    assert!(bob_at("carol one") < bob_at("carol two"));
    assert_eq!(visible.len(), 4);
}

#[test]
fn the_history_reports_the_current_delivery_state_and_not_the_one_at_write_time() {
    // AC11 asks for the live truth. A direct message is written `pending` and
    // becomes something else later, so the read must come from the live
    // conversation rather than from the append-only mirror.
    use crate::ports::InboundEnvelopePort;

    let context = alice();
    let outcome = context
        .send_direct(test_peers::bob(), "landed")
        .expect("sent");
    context
        .context
        .inbound()
        .message_delivered(outcome.sent.id)
        .expect("acknowledged");

    let history = context.history(ConversationId::Direct(test_peers::bob()));

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].delivery_state(), DeliveryState::Delivered);
    assert_eq!(
        context.mirrored(ConversationId::Direct(test_peers::bob()))[0].delivery_state(),
        DeliveryState::Pending,
        "the mirror is append-only, which is exactly why it is not the read model"
    );
}

#[test]
fn reading_the_same_conversation_repeatedly_returns_the_same_thing() {
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 1, "stable", CLAIMED_AT))
        .expect("applied");
    let events_before = context.events().len();

    let first = context.history(ConversationId::Broadcast);
    let second = context.history(ConversationId::Broadcast);

    assert_eq!(first, second);
    assert_eq!(
        context.events().len(),
        events_before,
        "a read announces nothing"
    );
}
