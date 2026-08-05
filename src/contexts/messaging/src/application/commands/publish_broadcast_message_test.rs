use std::sync::Arc;

use shared_types::PayloadKind;

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, DeliveryState};
use crate::ports::port_fakes::{FailingTransport, RecordingTransport};
use crate::ports::{
    MessagePayload, MessageTransportError, MessageTransportPort, MessagingCommandError,
    MessagingQueryPort,
};
use crate::test_peers;

fn with_recording_transport() -> (TestContext, Arc<RecordingTransport>) {
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    (context, transport)
}

#[test]
fn a_broadcast_is_released_to_the_topic_and_recorded_as_published() {
    let (context, transport) = with_recording_transport();

    let outcome = context
        .publish_broadcast("hello everyone")
        .expect("the publish completes");

    let published = transport.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].author, test_peers::alice());
    assert_eq!(published[0].kind, PayloadKind::BroadcastMessage);
    assert_eq!(outcome.delivery, DeliveryState::Published);
    assert!(
        transport.sent_direct().is_empty(),
        "a broadcast has no recipient"
    );
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["hello everyone"]
    );
}

#[test]
fn a_broadcast_is_never_pending_and_never_delivered() {
    // D3/AC10: gossip has no recipient set and no acknowledgement, so
    // "delivered" would be a claim this peer cannot make.
    let (context, _) = with_recording_transport();

    let outcome = context
        .publish_broadcast("said to the network")
        .expect("published");

    assert!(!outcome.is_pending());
    assert_eq!(outcome.failure_reason(), None);
    assert_eq!(
        context
            .context
            .queries()
            .delivery_state(outcome.sent.id)
            .expect("applied"),
        DeliveryState::Published
    );
}

#[test]
fn a_broadcast_the_topic_refused_is_not_recorded_locally() {
    // A broadcast has no failed delivery state, so a message the topic never
    // accepted must not be left behind claiming it was published. This is the
    // exact opposite of the direct path, for the same reason: the local record
    // must never claim more than is true.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(
            Arc::new(FailingTransport(MessageTransportError::Unavailable))
                as Arc<dyn MessageTransportPort + Send + Sync>,
        )
        .build();

    let outcome = context.publish_broadcast("never left the machine");

    assert!(matches!(
        outcome,
        Err(MessagingCommandError::Transport(
            MessageTransportError::Unavailable
        ))
    ));
    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
    assert_eq!(context.events(), Vec::new());
    assert_eq!(
        context.context.queries().conversations().expect("log"),
        Vec::new()
    );
}

#[test]
fn the_broadcast_counter_is_separate_from_every_direct_one() {
    let (context, transport) = with_recording_transport();

    context
        .publish_broadcast("first to all")
        .expect("published");
    context
        .send_direct(test_peers::bob(), "first to bob")
        .expect("sent");
    context
        .publish_broadcast("second to all")
        .expect("published");

    let broadcast_numbers: Vec<_> = transport
        .published()
        .iter()
        .map(|envelope| {
            MessagePayload::decode(&envelope.payload)
                .expect("decodes")
                .sequence()
        })
        .collect();
    let direct_numbers: Vec<_> = transport
        .sent_direct()
        .iter()
        .map(|(_, envelope)| {
            MessagePayload::decode(&envelope.payload)
                .expect("decodes")
                .sequence()
        })
        .collect();

    assert_eq!(broadcast_numbers, vec![sequence(1), sequence(2)]);
    assert_eq!(direct_numbers, vec![sequence(1)]);
}

#[test]
fn a_published_broadcast_is_announced_once() {
    let (context, _) = with_recording_transport();

    let outcome = context.publish_broadcast("announce me").expect("published");

    let sent_events: Vec<_> = context
        .events()
        .into_iter()
        .filter(|event| matches!(event, MessagingEvent::MessageSent(_)))
        .collect();
    assert_eq!(sent_events, vec![MessagingEvent::MessageSent(outcome.sent)]);
}
