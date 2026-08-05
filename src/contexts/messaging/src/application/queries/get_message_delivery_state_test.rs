use crate::application::test_context::{TestContext, TestContextBuilder, broadcast_from, sequence};
use crate::domain::{ConversationId, DeliveryState, MessageId, Millis};
use crate::ports::MessagingQueryPort;
use crate::test_peers;

const CLAIMED_AT: Millis = Millis::from_millis(7);

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

fn state_of(context: &TestContext, id: MessageId) -> Option<DeliveryState> {
    context.context.queries().delivery_state(id)
}

#[test]
fn a_direct_message_this_peer_sent_starts_pending() {
    let context = alice();

    let outcome = context
        .send_direct(test_peers::bob(), "waiting")
        .expect("sent");

    assert_eq!(
        state_of(&context, outcome.sent.id),
        Some(DeliveryState::Pending)
    );
}

#[test]
fn a_direct_message_this_peer_received_is_already_delivered() {
    // It is in hand, so "pending" would be false.
    let context = alice();
    context
        .accept(crate::application::test_context::direct_from(
            test_peers::bob(),
            1,
            "in hand",
            CLAIMED_AT,
        ))
        .expect("applied");

    let id = MessageId::new(
        test_peers::bob(),
        ConversationId::Direct(test_peers::bob()),
        sequence(1),
    );

    assert_eq!(state_of(&context, id), Some(DeliveryState::Delivered));
}

#[test]
fn a_broadcast_message_is_published_whoever_wrote_it() {
    let context = alice();
    let mine = context.publish_broadcast("mine").expect("published");
    context
        .accept(broadcast_from(test_peers::bob(), 1, "theirs", CLAIMED_AT))
        .expect("applied");
    let theirs = MessageId::new(test_peers::bob(), ConversationId::Broadcast, sequence(1));

    assert_eq!(
        state_of(&context, mine.sent.id),
        Some(DeliveryState::Published)
    );
    assert_eq!(state_of(&context, theirs), Some(DeliveryState::Published));
}

#[test]
fn a_message_this_peer_does_not_hold_has_no_state() {
    let context = alice();
    let unknown = MessageId::new(
        test_peers::dave(),
        ConversationId::Direct(test_peers::dave()),
        sequence(3),
    );

    assert_eq!(state_of(&context, unknown), None);
}

#[test]
fn a_buffered_message_has_no_state_because_it_is_not_in_the_conversation_yet() {
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 6, "early", CLAIMED_AT))
        .expect("buffered");
    let held = MessageId::new(test_peers::bob(), ConversationId::Broadcast, sequence(6));

    assert_eq!(state_of(&context, held), None);
}

#[test]
fn asking_about_an_unknown_message_opens_no_conversation() {
    let context = alice();
    let unknown = MessageId::new(
        test_peers::dave(),
        ConversationId::Direct(test_peers::dave()),
        sequence(1),
    );

    for _ in 0..3 {
        assert_eq!(state_of(&context, unknown), None);
    }

    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
    assert_eq!(context.events(), Vec::new());
}
