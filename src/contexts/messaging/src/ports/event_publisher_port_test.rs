use crate::domain::events::{MessageDuplicateIgnored, MessageSent, MessagingEvent};
use crate::domain::{ConversationId, MessageId, Millis, SequenceNumber};
use crate::ports::port_fakes::{RecordingEventPublisher, UnavailableEventPublisher};
use crate::ports::{EventPublisherError, EventPublisherPort};
use crate::test_peers;

fn id(sequence: u64) -> MessageId {
    MessageId::new(
        test_peers::alice(),
        ConversationId::Broadcast,
        SequenceNumber::new(sequence).expect("positive"),
    )
}

fn sent(sequence: u64) -> MessagingEvent {
    MessageSent {
        id: id(sequence),
        claimed_sent_at: Millis::from_millis(1),
    }
    .into()
}

#[test]
fn the_port_is_object_safe_so_one_publisher_can_be_shared() {
    let publisher = RecordingEventPublisher::default();
    let port: &dyn EventPublisherPort = &publisher;

    assert!(port.publish(sent(1)).is_ok());
}

#[test]
fn events_are_published_in_the_order_they_are_handed_over() {
    // A `MessageReceived` overtaking the one before it would show a
    // conversation out of order (AC8), which is the one thing the sequencing
    // rules exist to prevent.
    let publisher = RecordingEventPublisher::default();
    let duplicate = MessagingEvent::from(MessageDuplicateIgnored { id: id(1) });

    publisher.publish(sent(1)).expect("accepted");
    publisher.publish(duplicate).expect("accepted");
    publisher.publish(sent(2)).expect("accepted");

    assert_eq!(publisher.published(), [sent(1), duplicate, sent(2)]);
}

#[test]
fn an_unavailable_publisher_reports_a_typed_error() {
    let publisher = UnavailableEventPublisher;

    assert_eq!(
        publisher.publish(sent(1)),
        Err(EventPublisherError::Unavailable)
    );
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        EventPublisherError::Unavailable.to_string(),
        "the event publisher is not available"
    );
}
