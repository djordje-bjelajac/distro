use std::sync::Arc;

use shared_types::{PeerConnected, PeerDisconnected};

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, DeliveryFailure, DeliveryState};
use crate::ports::port_fakes::{InMemorySequenceCounter, RecordingTransport};
use crate::ports::{
    MessagePayload, MessageTransportPort, MessagingQueryPort, PeerLifecyclePort,
    SequenceCounterPort,
};
use crate::test_peers;

fn with_recording_transport() -> (TestContext, Arc<RecordingTransport>) {
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    (context, transport)
}

fn disconnect(context: &TestContext, peer: shared_types::PeerId) -> Vec<DeliveryState> {
    context
        .context
        .lifecycle()
        .peer_disconnected(PeerDisconnected { peer })
        .expect("the disconnect is handled")
        .into_iter()
        .map(|change| change.to)
        .collect()
}

#[test]
fn a_disconnect_fails_that_peers_pending_directs() {
    // D10/AC11: a message handed to a transport whose session has died will not
    // arrive, and `pending` forever is silent loss wearing a spinner.
    let (context, _) = with_recording_transport();
    let first = context.send_direct(test_peers::bob(), "one").expect("sent");
    let second = context.send_direct(test_peers::bob(), "two").expect("sent");

    let states = disconnect(&context, test_peers::bob());

    assert_eq!(
        states,
        vec![
            DeliveryState::Failed(DeliveryFailure::SessionClosed),
            DeliveryState::Failed(DeliveryFailure::SessionClosed),
        ]
    );
    for id in [first.sent.id, second.sent.id] {
        assert_eq!(
            context.context.queries().delivery_state(id),
            Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
        );
    }
}

#[test]
fn every_failure_is_announced() {
    let (context, _) = with_recording_transport();
    context.send_direct(test_peers::bob(), "one").expect("sent");

    disconnect(&context, test_peers::bob());

    let changes: Vec<_> = context
        .events()
        .into_iter()
        .filter(|event| matches!(event, MessagingEvent::MessageDeliveryStateChanged(_)))
        .collect();
    assert_eq!(changes.len(), 1);
}

#[test]
fn a_disconnect_leaves_another_peers_messages_alone() {
    // A disconnect is news about one link, not about the network.
    let (context, _) = with_recording_transport();
    let to_bob = context
        .send_direct(test_peers::bob(), "for bob")
        .expect("sent");
    let to_carol = context
        .send_direct(test_peers::carol(), "for carol")
        .expect("sent");

    disconnect(&context, test_peers::bob());

    assert_eq!(
        context.context.queries().delivery_state(to_bob.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
    );
    assert_eq!(
        context.context.queries().delivery_state(to_carol.sent.id),
        Some(DeliveryState::Pending)
    );
}

#[test]
fn a_disconnect_leaves_broadcast_messages_alone() {
    // Broadcast messages are `Published`, and gossip has no session to lose.
    let (context, _) = with_recording_transport();
    let broadcast = context.publish_broadcast("to everyone").expect("published");

    disconnect(&context, test_peers::bob());

    assert_eq!(
        context.context.queries().delivery_state(broadcast.sent.id),
        Some(DeliveryState::Published)
    );
}

#[test]
fn a_message_already_delivered_is_not_failed_by_a_later_disconnect() {
    use crate::ports::InboundEnvelopePort;

    let (context, _) = with_recording_transport();
    let outcome = context
        .send_direct(test_peers::bob(), "landed")
        .expect("sent");
    context
        .context
        .inbound()
        .message_delivered(outcome.sent.id)
        .expect("acknowledged");

    let states = disconnect(&context, test_peers::bob());

    assert_eq!(states, Vec::new());
    assert_eq!(
        context.context.queries().delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
}

#[test]
fn a_disconnect_from_a_peer_this_instance_never_messaged_changes_nothing() {
    let (context, _) = with_recording_transport();

    let states = disconnect(&context, test_peers::dave());

    assert_eq!(states, Vec::new());
    assert_eq!(context.events(), Vec::new());
    assert_eq!(
        context.history(ConversationId::Direct(test_peers::dave())),
        Vec::new()
    );
}

#[test]
fn a_connect_opens_the_conversation_at_the_counters_mark() {
    // D12/AC16: the connect is where the outbound sequence is restored, so the
    // first message after a restart continues the run listeners already hold.
    let counter = Arc::new(InMemorySequenceCounter::restored_with([(
        ConversationId::Direct(test_peers::bob()),
        sequence(12),
    )]));
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_counter(Arc::clone(&counter) as Arc<dyn SequenceCounterPort + Send + Sync>)
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    context
        .context
        .lifecycle()
        .peer_connected(PeerConnected {
            peer: test_peers::bob(),
        })
        .expect("the connect is handled");
    context
        .send_direct(test_peers::bob(), "still here")
        .expect("sent");

    let (_, envelope) = &transport.sent_direct()[0];
    assert_eq!(
        MessagePayload::decode(&envelope.payload)
            .expect("decodes")
            .sequence(),
        sequence(13)
    );
}

#[test]
fn a_repeated_connect_changes_nothing() {
    let (context, transport) = with_recording_transport();
    context
        .send_direct(test_peers::bob(), "before")
        .expect("sent");

    for _ in 0..3 {
        context
            .context
            .lifecycle()
            .peer_connected(PeerConnected {
                peer: test_peers::bob(),
            })
            .expect("idempotent");
    }
    context
        .send_direct(test_peers::bob(), "after")
        .expect("sent");

    assert_eq!(
        context.visible_text(ConversationId::Direct(test_peers::bob())),
        vec!["before", "after"]
    );
    assert_eq!(transport.sent_direct().len(), 2);
}
