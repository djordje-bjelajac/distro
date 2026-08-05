use std::sync::Arc;

use messaging::domain::events::{
    GapCloseCause, MessageDuplicateIgnored, MessageGapClosed, MessageRejected, MessageSent,
    MessagingEvent, RejectionReason,
};
use messaging::domain::{ConversationId, MessageId, Millis, SequenceNumber};
use messaging::ports::EventPublisherPort;

use crate::composition::{Diagnostics, GapLedger, MessagingEventSink};
use crate::test_peers::{alice, bob};

fn sequence(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("a non-zero sequence")
}

fn wired() -> (Arc<GapLedger>, Arc<Diagnostics>, MessagingEventSink) {
    let gaps = Arc::new(GapLedger::new());
    let diagnostics = Arc::new(Diagnostics::default());
    let sink = MessagingEventSink::new(Arc::clone(&gaps), Arc::clone(&diagnostics));

    (gaps, diagnostics, sink)
}

#[test]
fn an_abandoned_gap_becomes_a_marker_and_two_numbers() {
    // AC15: the abandoned run is visible in the conversation *and* counted.
    let (gaps, diagnostics, sink) = wired();
    let closed = MessageGapClosed {
        conversation: ConversationId::Broadcast,
        author: alice(),
        from: sequence(3),
        to: sequence(6),
        cause: GapCloseCause::ToleranceElapsed,
    };

    sink.publish(MessagingEvent::MessageGapClosed(closed))
        .expect("the sink accepts");

    assert_eq!(gaps.of(ConversationId::Broadcast), vec![closed]);
    assert_eq!(diagnostics.gaps_abandoned(), 1);
    assert_eq!(diagnostics.messages_never_received(), 4);
}

#[test]
fn a_buffer_full_close_is_recorded_the_same_way() {
    // Both causes close the same gap; only the diagnostic tells a slow path
    // from a flooding peer.
    let (gaps, _diagnostics, sink) = wired();
    let closed = MessageGapClosed {
        conversation: ConversationId::Direct(bob()),
        author: bob(),
        from: sequence(2),
        to: sequence(2),
        cause: GapCloseCause::BufferFull,
    };

    sink.publish(MessagingEvent::MessageGapClosed(closed))
        .expect("the sink accepts");

    assert_eq!(gaps.of(ConversationId::Direct(bob())), vec![closed]);
}

#[test]
fn a_rejected_envelope_is_counted() {
    // AC6: refused content reaches no read model, so a counter is the only
    // place it can be seen at all.
    let (_gaps, diagnostics, sink) = wired();

    sink.publish(MessagingEvent::MessageRejected(MessageRejected {
        conversation: ConversationId::Broadcast,
        claimed_author: alice(),
        sequence: Some(sequence(1)),
        reason: RejectionReason::SignatureInvalid,
    }))
    .expect("the sink accepts");

    assert_eq!(diagnostics.envelopes_refused(), 1);
}

#[test]
fn a_duplicate_is_counted_and_changes_nothing_else() {
    let (gaps, diagnostics, sink) = wired();

    sink.publish(MessagingEvent::MessageDuplicateIgnored(
        MessageDuplicateIgnored {
            id: MessageId::new(alice(), ConversationId::Broadcast, sequence(1)),
        },
    ))
    .expect("the sink accepts");

    assert_eq!(diagnostics.duplicates_ignored(), 1);
    assert!(gaps.all().is_empty());
}

#[test]
fn an_applied_message_is_not_mirrored() {
    // The read model already has it; a second copy is a second thing that can
    // disagree.
    let (gaps, diagnostics, sink) = wired();

    sink.publish(MessagingEvent::MessageSent(MessageSent {
        id: MessageId::new(alice(), ConversationId::Broadcast, sequence(1)),
        claimed_sent_at: Millis::from_millis(5),
    }))
    .expect("the sink accepts");

    assert!(gaps.all().is_empty());
    assert_eq!(diagnostics.gaps_abandoned(), 0);
    assert_eq!(diagnostics.envelopes_refused(), 0);
    assert_eq!(diagnostics.duplicates_ignored(), 0);
}
