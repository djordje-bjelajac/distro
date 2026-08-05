use crate::domain::events::{
    GapCloseCause, MessageDeliveryStateChanged, MessageDuplicateIgnored, MessageGapClosed,
    MessageReceived, MessageRejected, MessageSent, MessagingEvent, RejectionReason,
};
use crate::domain::{ConversationId, DeliveryState, MessageId, Millis, SequenceNumber};
use crate::test_peers;

const AT: Millis = Millis::from_millis(9);

fn id() -> MessageId {
    MessageId::new(
        test_peers::bob(),
        ConversationId::Broadcast,
        SequenceNumber::FIRST,
    )
}

#[test]
fn every_event_converts_into_the_published_union() {
    let sent = MessageSent {
        id: id(),
        claimed_sent_at: AT,
    };
    let received = MessageReceived {
        id: id(),
        claimed_sent_at: AT,
    };
    let duplicate = MessageDuplicateIgnored { id: id() };
    let rejected = MessageRejected {
        conversation: ConversationId::Broadcast,
        claimed_author: test_peers::bob(),
        sequence: Some(SequenceNumber::FIRST),
        reason: RejectionReason::SignatureInvalid,
    };
    let changed = MessageDeliveryStateChanged {
        id: id(),
        from: DeliveryState::Pending,
        to: DeliveryState::Delivered,
    };
    let gap_closed = MessageGapClosed {
        conversation: ConversationId::Broadcast,
        author: test_peers::bob(),
        from: SequenceNumber::FIRST,
        to: SequenceNumber::FIRST,
        cause: GapCloseCause::ToleranceElapsed,
    };

    assert_eq!(
        MessagingEvent::from(sent),
        MessagingEvent::MessageSent(sent)
    );
    assert_eq!(
        MessagingEvent::from(received),
        MessagingEvent::MessageReceived(received)
    );
    assert_eq!(
        MessagingEvent::from(duplicate),
        MessagingEvent::MessageDuplicateIgnored(duplicate)
    );
    assert_eq!(
        MessagingEvent::from(rejected),
        MessagingEvent::MessageRejected(rejected)
    );
    assert_eq!(
        MessagingEvent::from(changed),
        MessagingEvent::MessageDeliveryStateChanged(changed)
    );
    assert_eq!(
        MessagingEvent::from(gap_closed),
        MessagingEvent::MessageGapClosed(gap_closed)
    );
}

#[test]
fn a_rejection_before_verification_has_no_sequence_to_report() {
    // Invariant 4: until a signature verifies there is no established author
    // and no trustworthy sequence, so the event says so instead of inventing
    // one.
    let rejected = MessageRejected {
        conversation: ConversationId::Broadcast,
        claimed_author: test_peers::bob(),
        sequence: None,
        reason: RejectionReason::MalformedPayload,
    };

    assert_eq!(rejected.sequence, None);
}

#[test]
fn rejection_reasons_render_their_cause() {
    assert_eq!(
        RejectionReason::SignatureInvalid.to_string(),
        "the envelope signature did not verify against its author"
    );
    assert_eq!(
        RejectionReason::ArrivedAfterGapClosed.to_string(),
        "the message arrived after its gap had been abandoned"
    );
}

#[test]
fn a_gap_close_names_which_of_the_two_triggers_fired() {
    // AC15: the abandonment is a diagnostic, and "why" is half of it — a buffer
    // filled by one author reads very differently from a window that elapsed.
    assert_eq!(
        GapCloseCause::ToleranceElapsed.to_string(),
        "the gap-tolerance window elapsed without the missing messages arriving"
    );
    assert_eq!(
        GapCloseCause::BufferFull.to_string(),
        "the author's out-of-order buffer was full"
    );
}

#[test]
fn an_abandoned_range_names_both_of_its_ends() {
    // A one-message gap has from == to; nothing infers the range from the
    // messages that follow it.
    let single = MessageGapClosed {
        conversation: ConversationId::Direct(test_peers::bob()),
        author: test_peers::bob(),
        from: SequenceNumber::FIRST,
        to: SequenceNumber::FIRST,
        cause: GapCloseCause::BufferFull,
    };

    assert_eq!(single.from, single.to);
    assert_eq!(single.author, test_peers::bob());
}
