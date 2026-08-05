use std::sync::Arc;

use shared_types::PayloadKind;

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, DeliveryFailure, DeliveryState};
use crate::ports::port_fakes::{
    FailingSigner, FailingTransport, InMemorySequenceCounter, RecordingTransport,
    UnavailableSequenceCounter,
};
use crate::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, MessagePayload, MessageTransportError,
    MessageTransportPort, MessagingCommandError, MessagingQueryPort, SequenceCounterPort,
};
use crate::test_peers;

fn with_recording_transport() -> (TestContext, Arc<RecordingTransport>) {
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    (context, transport)
}

fn failing_with(error: MessageTransportError) -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice())
        .with_transport(
            Arc::new(FailingTransport(error)) as Arc<dyn MessageTransportPort + Send + Sync>
        )
        .build()
}

fn direct_with_bob() -> ConversationId {
    ConversationId::Direct(test_peers::bob())
}

#[test]
fn a_direct_message_is_signed_and_handed_to_the_transport() {
    let (context, transport) = with_recording_transport();

    let outcome = context
        .send_direct(test_peers::bob(), "hello bob")
        .expect("the send completes");

    let sent = transport.sent_direct();
    assert_eq!(sent.len(), 1);
    let (recipient, envelope) = &sent[0];
    assert_eq!(*recipient, test_peers::bob());
    assert_eq!(envelope.author, test_peers::alice());
    assert_eq!(envelope.kind, PayloadKind::DirectMessage);
    assert_eq!(outcome.delivery, DeliveryState::Pending);
    assert!(transport.published().is_empty(), "no broadcast was made");
}

#[test]
fn the_envelope_carries_the_body_the_caller_wrote() {
    let (context, transport) = with_recording_transport();

    context
        .send_direct(test_peers::bob(), "  padded  ")
        .expect("send");

    let (_, envelope) = &transport.sent_direct()[0];
    let payload = MessagePayload::decode(&envelope.payload).expect("our own payload decodes");
    assert_eq!(payload.body().as_str(), "padded");
    assert_eq!(payload.sequence(), sequence(1));
}

#[test]
fn the_wire_sequence_comes_from_the_counter_and_not_from_the_aggregate() {
    // D12/AC16. The counter was restored from a store holding what a previous
    // process issued; the conversation holds no messages at all (D7). If the
    // aggregate's own mark decided, this message would go out as #1.
    let counter = Arc::new(InMemorySequenceCounter::restored_with([(
        direct_with_bob(),
        sequence(7),
    )]));
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_counter(Arc::clone(&counter) as Arc<dyn SequenceCounterPort + Send + Sync>)
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    let outcome = context
        .send_direct(test_peers::bob(), "after a restart")
        .expect("send");

    let (_, envelope) = &transport.sent_direct()[0];
    let payload = MessagePayload::decode(&envelope.payload).expect("decodes");
    assert_eq!(payload.sequence(), sequence(8));
    assert_eq!(outcome.sent.id.sequence(), sequence(8));
    assert_eq!(
        counter.last_issued(direct_with_bob()).expect("counter"),
        Some(sequence(8)),
        "the advance is recorded before the number is used"
    );
}

#[test]
fn successive_sends_take_successive_numbers() {
    let (context, transport) = with_recording_transport();

    for text in ["one", "two", "three"] {
        context.send_direct(test_peers::bob(), text).expect("send");
    }

    let numbers: Vec<_> = transport
        .sent_direct()
        .iter()
        .map(|(_, envelope)| {
            MessagePayload::decode(&envelope.payload)
                .expect("decodes")
                .sequence()
        })
        .collect();
    assert_eq!(numbers, vec![sequence(1), sequence(2), sequence(3)]);
}

#[test]
fn a_transport_failure_leaves_the_message_visibly_failed() {
    // AC11/D10: silent loss is not a state. The message exists, it is visible,
    // and it names the reason the user needs to decide what to do next.
    let context = failing_with(MessageTransportError::PeerUnreachable);

    let outcome = context
        .send_direct(test_peers::bob(), "into the void")
        .expect("a refused send still produces a message");

    assert_eq!(
        outcome.delivery,
        DeliveryState::Failed(DeliveryFailure::PeerUnreachable)
    );
    assert_eq!(
        outcome.failure_reason(),
        Some(DeliveryFailure::PeerUnreachable)
    );
    assert_eq!(
        context.visible_text(direct_with_bob()),
        vec!["into the void"]
    );
    assert_eq!(
        context
            .context
            .queries()
            .delivery_state(outcome.sent.id)
            .expect("the message is applied"),
        DeliveryState::Failed(DeliveryFailure::PeerUnreachable)
    );
}

#[test]
fn every_transport_failure_reaches_the_user_as_its_own_delivery_reason() {
    // The mapping is what makes AC11 honest: "failed" alone would be silent
    // loss with extra steps.
    let cases = [
        (
            MessageTransportError::Unavailable,
            DeliveryFailure::TransportUnavailable,
        ),
        (
            MessageTransportError::PeerUnreachable,
            DeliveryFailure::PeerUnreachable,
        ),
        (
            MessageTransportError::NoRelayAvailable,
            DeliveryFailure::NoRelayAvailable,
        ),
        (
            MessageTransportError::SessionClosed,
            DeliveryFailure::SessionClosed,
        ),
        (
            MessageTransportError::NotAcknowledged,
            DeliveryFailure::RetriesExhausted,
        ),
    ];

    for (transport_error, expected) in cases {
        let context = failing_with(transport_error);

        let outcome = context
            .send_direct(test_peers::bob(), "attempt")
            .expect("sent");

        assert_eq!(outcome.delivery, DeliveryState::Failed(expected));
    }
}

#[test]
fn a_failed_send_announces_both_the_message_and_its_failure_in_that_order() {
    let context = failing_with(MessageTransportError::SessionClosed);

    let outcome = context
        .send_direct(test_peers::bob(), "unlucky")
        .expect("sent");

    let events = context.events();
    assert!(matches!(events[0], MessagingEvent::MessageSent(sent) if sent.id == outcome.sent.id));
    assert!(matches!(
        events[1],
        MessagingEvent::MessageDeliveryStateChanged(changed)
            if changed.from == DeliveryState::Pending
                && changed.to == DeliveryState::Failed(DeliveryFailure::SessionClosed)
    ));
}

#[test]
fn a_signing_failure_records_no_local_message_at_all() {
    // An unsigned envelope has no author (invariant 4) and no peer would take
    // it, so displaying it locally would promise something that cannot happen.
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_signer(Arc::new(FailingSigner(EnvelopeSignerError::KeyUnavailable))
            as Arc<dyn EnvelopeSignerPort + Send + Sync>)
        .build();

    let outcome = context.send_direct(test_peers::bob(), "never signed");

    assert!(matches!(
        outcome,
        Err(MessagingCommandError::Signer(
            EnvelopeSignerError::KeyUnavailable
        ))
    ));
    assert_eq!(context.history(direct_with_bob()), Vec::new());
    assert_eq!(context.events(), Vec::new());
}

#[test]
fn a_direct_send_never_touches_the_broadcast_channel_or_another_peer() {
    let (context, transport) = with_recording_transport();

    context
        .send_direct(test_peers::bob(), "private")
        .expect("send");
    context
        .send_direct(test_peers::carol(), "also private")
        .expect("send");

    assert!(transport.published().is_empty());
    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
    assert_eq!(context.visible_text(direct_with_bob()), vec!["private"]);
    assert_eq!(
        context.visible_text(ConversationId::Direct(test_peers::carol())),
        vec!["also private"]
    );
}

#[test]
fn a_counter_that_cannot_issue_stops_the_send_before_anything_happens() {
    // `issue_next` records the advance before it returns; a number that could
    // not be recorded must not be used, because it would be re-issued after a
    // crash — the exact failure the counter exists to prevent (D12).
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_counter(Arc::new(UnavailableSequenceCounter))
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    let outcome = context.send_direct(test_peers::bob(), "blocked");

    assert!(matches!(outcome, Err(MessagingCommandError::Sequence(_))));
    assert!(transport.sent_direct().is_empty());
}
