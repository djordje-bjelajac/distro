use crate::domain::{
    ConversationId, DeliveryFailure, DeliveryState, Message, MessageBody, MessageId, Millis,
    SequenceNumber,
};
use crate::test_peers;

const SENT_AT: Millis = Millis::from_millis(1_700_000_000_000);

fn body(text: &str) -> MessageBody {
    MessageBody::new(text).expect("test body is valid")
}

fn id(conversation: ConversationId) -> MessageId {
    MessageId::new(test_peers::bob(), conversation, SequenceNumber::FIRST)
}

#[test]
fn a_message_exposes_the_three_parts_of_its_identifier() {
    let conversation = ConversationId::Direct(test_peers::alice());
    let message = Message::outbound(id(conversation), body("hi"), SENT_AT);

    assert_eq!(message.id(), id(conversation));
    assert_eq!(message.author(), test_peers::bob());
    assert_eq!(message.conversation(), conversation);
    assert_eq!(message.sequence(), SequenceNumber::FIRST);
    assert_eq!(message.body().as_str(), "hi");
}

#[test]
fn the_sent_at_instant_is_the_authors_claim_kept_verbatim() {
    // Display only: it is never compared, sorted by, or validated. A peer with
    // a wrong or hostile clock can claim anything, and ordering is unaffected
    // because ordering is the sequence number's job (invariant 5, AC8).
    let absurd = Millis::from_millis(u64::MAX);
    let message = Message::received(id(ConversationId::Broadcast), body("hi"), absurd);

    assert_eq!(message.claimed_sent_at(), absurd);
}

// --------------------------------------------- which lifecycle a message enters

#[test]
fn a_locally_sent_direct_message_starts_pending() {
    let message = Message::outbound(
        id(ConversationId::Direct(test_peers::alice())),
        body("hi"),
        SENT_AT,
    );

    assert_eq!(message.delivery_state(), DeliveryState::Pending);
}

#[test]
fn a_locally_sent_broadcast_message_is_published() {
    // Gossip has no recipient set to acknowledge (D3, AC10), so publishing is
    // the whole of what this peer knows.
    let message = Message::outbound(id(ConversationId::Broadcast), body("hi"), SENT_AT);

    assert_eq!(message.delivery_state(), DeliveryState::Published);
}

#[test]
fn a_received_direct_message_is_already_delivered() {
    let message = Message::received(
        id(ConversationId::Direct(test_peers::alice())),
        body("hi"),
        SENT_AT,
    );

    assert_eq!(message.delivery_state(), DeliveryState::Delivered);
}

#[test]
fn a_received_broadcast_message_is_published() {
    let message = Message::received(id(ConversationId::Broadcast), body("hi"), SENT_AT);

    assert_eq!(message.delivery_state(), DeliveryState::Published);
}

// ------------------------------------------------------------- transitions

#[test]
fn a_pending_message_records_delivery_and_reports_the_change() {
    let mut message = Message::outbound(
        id(ConversationId::Direct(test_peers::alice())),
        body("hi"),
        SENT_AT,
    );

    let change = message.mark_delivered().expect("pending may be delivered");

    assert_eq!(change.id, message.id());
    assert_eq!(change.from, DeliveryState::Pending);
    assert_eq!(change.to, DeliveryState::Delivered);
    assert_eq!(message.delivery_state(), DeliveryState::Delivered);
}

#[test]
fn a_pending_message_records_failure_with_its_reason() {
    let mut message = Message::outbound(
        id(ConversationId::Direct(test_peers::alice())),
        body("hi"),
        SENT_AT,
    );

    let change = message
        .mark_failed(DeliveryFailure::NoRelayAvailable)
        .expect("pending may fail");

    assert_eq!(
        change.to,
        DeliveryState::Failed(DeliveryFailure::NoRelayAvailable)
    );
    assert_eq!(
        message.delivery_state().failure_reason(),
        Some(DeliveryFailure::NoRelayAvailable)
    );
}

#[test]
fn a_rejected_transition_leaves_the_message_untouched() {
    let mut message = Message::outbound(id(ConversationId::Broadcast), body("hi"), SENT_AT);

    assert!(message.mark_delivered().is_err());
    assert_eq!(message.delivery_state(), DeliveryState::Published);
}
