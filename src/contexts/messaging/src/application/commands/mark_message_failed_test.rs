use shared_types::PeerDisconnected;

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationError, ConversationId, DeliveryFailure, DeliveryState, MessageId};
use crate::ports::{
    InboundEnvelopePort, MessagingCommandError, MessagingQueryPort, PeerLifecyclePort,
};
use crate::test_peers;

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice()).build()
}

/// Every delivery-state change this context announced, in order.
fn announced(context: &TestContext) -> Vec<MessagingEvent> {
    context
        .events()
        .into_iter()
        .filter(|event| matches!(event, MessagingEvent::MessageDeliveryStateChanged(_)))
        .collect()
}

#[test]
fn a_refusal_arriving_after_the_send_returned_reaches_failed() {
    // OP-12a/AC11/D10. `MessageTransportPort::send_direct` returned `Ok`
    // because the request was *queued*; the refusal came back afterwards as a
    // network event. Nothing here closes the session, so nothing else in this
    // context could move this message off `Pending` — which is the silent-loss
    // shape AC11 forbids.
    let context = alice();
    let outcome = context
        .send_direct(test_peers::bob(), "queued, then refused")
        .expect("sent");
    assert_eq!(outcome.delivery, DeliveryState::Pending);

    let change = context
        .context
        .inbound()
        .message_delivery_failed(outcome.sent.id, DeliveryFailure::RetriesExhausted)
        .expect("the refusal is recorded");

    assert_eq!(change.id, outcome.sent.id);
    assert_eq!(change.from, DeliveryState::Pending);
    assert_eq!(
        change.to,
        DeliveryState::Failed(DeliveryFailure::RetriesExhausted)
    );
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::RetriesExhausted))
    );
}

#[test]
fn only_the_refused_message_fails() {
    // The distinction from `peer_disconnected`, which fails *every* pending
    // direct to a peer: one refusal is news about one message.
    let context = alice();
    let refused = context.send_direct(test_peers::bob(), "one").expect("sent");
    let still_pending = context.send_direct(test_peers::bob(), "two").expect("sent");
    let to_carol = context
        .send_direct(test_peers::carol(), "for carol")
        .expect("sent");

    context
        .context
        .inbound()
        .message_delivery_failed(refused.sent.id, DeliveryFailure::PeerUnreachable)
        .expect("the refusal is recorded");

    assert_eq!(
        context.context.queries().delivery_state(refused.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::PeerUnreachable))
    );
    assert_eq!(
        context
            .context
            .queries()
            .delivery_state(still_pending.sent.id),
        Some(DeliveryState::Pending)
    );
    assert_eq!(
        context.context.queries().delivery_state(to_carol.sent.id),
        Some(DeliveryState::Pending)
    );
}

#[test]
fn a_later_disconnect_still_fails_what_is_left_pending() {
    // The one-message path and the whole-session path (D10) compose: the
    // already-failed message keeps its stated reason, and the survivor gets the
    // disconnect's.
    let context = alice();
    let refused = context.send_direct(test_peers::bob(), "one").expect("sent");
    let survivor = context.send_direct(test_peers::bob(), "two").expect("sent");
    context
        .context
        .inbound()
        .message_delivery_failed(refused.sent.id, DeliveryFailure::NoRelayAvailable)
        .expect("the refusal is recorded");

    let changes = context
        .context
        .lifecycle()
        .peer_disconnected(PeerDisconnected {
            peer: test_peers::bob(),
        })
        .expect("the disconnect is handled");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id, survivor.sent.id);
    assert_eq!(
        context.context.queries().delivery_state(refused.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::NoRelayAvailable))
    );
    assert_eq!(
        context.context.queries().delivery_state(survivor.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
    );
}

#[test]
fn a_broadcast_message_has_no_delivery_to_fail() {
    // Gossip has no recipient set and no acknowledgement, so it has no failure
    // either (D3). The aggregate decides this; nothing here reinterprets it.
    let context = alice();
    let outcome = context.publish_broadcast("to everyone").expect("published");

    let failed = context
        .context
        .inbound()
        .message_delivery_failed(outcome.sent.id, DeliveryFailure::PeerUnreachable);

    assert_eq!(
        failed,
        Err(MessagingCommandError::Conversation(
            ConversationError::InvalidDeliveryTransition {
                from: DeliveryState::Published,
                to: DeliveryState::Failed(DeliveryFailure::PeerUnreachable),
            }
        ))
    );
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Published)
    );
}

#[test]
fn a_failure_for_a_message_this_peer_does_not_hold_is_a_typed_error() {
    let context = alice();
    let unknown = MessageId::new(
        test_peers::alice(),
        ConversationId::Direct(test_peers::bob()),
        sequence(9),
    );

    let failed = context
        .context
        .inbound()
        .message_delivery_failed(unknown, DeliveryFailure::SessionClosed);

    assert_eq!(
        failed,
        Err(MessagingCommandError::Conversation(
            ConversationError::UnknownMessage
        ))
    );
}

#[test]
fn a_failure_does_not_bring_a_conversation_into_existence() {
    // A stray report about a peer this instance never messaged must not
    // populate the conversation list.
    let context = alice();
    let unknown = MessageId::new(
        test_peers::alice(),
        ConversationId::Direct(test_peers::dave()),
        sequence(1),
    );

    let _ = context
        .context
        .inbound()
        .message_delivery_failed(unknown, DeliveryFailure::TransportUnavailable);

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
fn a_late_failure_cannot_overturn_a_delivered_message() {
    // Terminal states are terminal: the user was already told this landed.
    let context = alice();
    let outcome = context
        .send_direct(test_peers::bob(), "landed")
        .expect("sent");
    context
        .context
        .inbound()
        .message_delivered(outcome.sent.id)
        .expect("acknowledged");

    let failed = context
        .context
        .inbound()
        .message_delivery_failed(outcome.sent.id, DeliveryFailure::RetriesExhausted);

    assert_eq!(
        failed,
        Err(MessagingCommandError::Conversation(
            ConversationError::InvalidDeliveryTransition {
                from: DeliveryState::Delivered,
                to: DeliveryState::Failed(DeliveryFailure::RetriesExhausted),
            }
        ))
    );
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
}

#[test]
fn the_transition_is_announced_exactly_once() {
    // A transport may report the same refusal twice — a retry cycle ending and
    // a stream resetting are two events about one message. The aggregate
    // refuses the repeat, so nothing is announced a second time and no consumer
    // sees the same message fail twice.
    let context = alice();
    let outcome = context.send_direct(test_peers::bob(), "one").expect("sent");

    let change = context
        .context
        .inbound()
        .message_delivery_failed(outcome.sent.id, DeliveryFailure::NoRelayAvailable)
        .expect("the refusal is recorded");
    let repeated = context
        .context
        .inbound()
        .message_delivery_failed(outcome.sent.id, DeliveryFailure::NoRelayAvailable);

    assert_eq!(
        repeated,
        Err(MessagingCommandError::Conversation(
            ConversationError::InvalidDeliveryTransition {
                from: DeliveryState::Failed(DeliveryFailure::NoRelayAvailable),
                to: DeliveryState::Failed(DeliveryFailure::NoRelayAvailable),
            }
        ))
    );
    assert_eq!(
        announced(&context),
        vec![MessagingEvent::MessageDeliveryStateChanged(change)]
    );
}

#[test]
fn the_reason_the_transport_gave_is_the_reason_recorded() {
    // The reason is the transport's to state and the aggregate's to record.
    // Nothing in this layer reinterprets, defaults, or collapses it (AC11).
    for reason in DeliveryFailure::ALL {
        let context = alice();
        let outcome = context.send_direct(test_peers::bob(), "one").expect("sent");

        let change = context
            .context
            .inbound()
            .message_delivery_failed(outcome.sent.id, reason)
            .expect("the refusal is recorded");

        assert_eq!(change.to, DeliveryState::Failed(reason));
        assert_eq!(
            context.context.queries().delivery_state(outcome.sent.id),
            Some(DeliveryState::Failed(reason))
        );
    }
}
