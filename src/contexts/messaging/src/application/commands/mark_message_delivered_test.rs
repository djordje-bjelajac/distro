use std::sync::Arc;

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationError, ConversationId, DeliveryFailure, DeliveryState, MessageId};
use crate::ports::port_fakes::FailingTransport;
use crate::ports::{
    InboundEnvelopePort, MessageTransportError, MessageTransportPort, MessagingCommandError,
    MessagingQueryPort,
};
use crate::test_peers;

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

#[test]
fn acknowledging_a_pending_direct_message_reports_the_transition() {
    let context = alice();
    let outcome = context
        .send_direct(test_peers::bob(), "did it land?")
        .expect("sent");

    let change = context
        .context
        .inbound()
        .message_delivered(outcome.sent.id)
        .expect("acknowledged");

    assert_eq!(change.id, outcome.sent.id);
    assert_eq!(change.from, DeliveryState::Pending);
    assert_eq!(change.to, DeliveryState::Delivered);
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
    assert!(
        context
            .events()
            .contains(&MessagingEvent::MessageDeliveryStateChanged(change))
    );
}

#[test]
fn an_acknowledgement_for_a_message_this_peer_does_not_hold_is_refused() {
    let context = alice();
    let unknown = MessageId::new(
        test_peers::alice(),
        ConversationId::Direct(test_peers::bob()),
        sequence(9),
    );

    let outcome = context.context.inbound().message_delivered(unknown);

    assert_eq!(
        outcome,
        Err(MessagingCommandError::Conversation(
            ConversationError::UnknownMessage
        ))
    );
}

#[test]
fn an_acknowledgement_does_not_bring_a_conversation_into_existence() {
    // A stray acknowledgement from a peer this instance never messaged must not
    // populate the conversation list.
    let context = alice();
    let unknown = MessageId::new(
        test_peers::alice(),
        ConversationId::Direct(test_peers::dave()),
        sequence(1),
    );

    let _ = context.context.inbound().message_delivered(unknown);

    assert_eq!(
        context.history(ConversationId::Direct(test_peers::dave())),
        Vec::new()
    );
    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
}

#[test]
fn a_late_acknowledgement_cannot_resurrect_a_failed_message() {
    // Terminal states are terminal: the user was already told this failed.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(
            Arc::new(FailingTransport(MessageTransportError::SessionClosed))
                as Arc<dyn MessageTransportPort + Send + Sync>,
        )
        .build();
    let outcome = context
        .send_direct(test_peers::bob(), "too late")
        .expect("sent");

    let acknowledged = context.context.inbound().message_delivered(outcome.sent.id);

    assert!(matches!(
        acknowledged,
        Err(MessagingCommandError::Conversation(
            ConversationError::InvalidDeliveryTransition { .. }
        ))
    ));
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
    );
}

#[test]
fn a_broadcast_message_has_no_acknowledgement_to_record() {
    // Gossip has no recipient set, so anything claiming one is wrong (D3).
    let context = alice();
    let outcome = context.publish_broadcast("to everyone").expect("published");

    let acknowledged = context.context.inbound().message_delivered(outcome.sent.id);

    assert!(matches!(
        acknowledged,
        Err(MessagingCommandError::Conversation(
            ConversationError::InvalidDeliveryTransition { .. }
        ))
    ));
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Published)
    );
}
