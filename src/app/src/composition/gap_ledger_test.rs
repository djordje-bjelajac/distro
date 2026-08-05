use messaging::domain::events::{GapCloseCause, MessageGapClosed};
use messaging::domain::{ConversationId, SequenceNumber};

use crate::composition::{GapLedger, abandoned_span};
use crate::test_peers::{alice, bob};

fn gap(
    conversation: ConversationId,
    author: shared_types::PeerId,
    from: u64,
    to: u64,
) -> MessageGapClosed {
    MessageGapClosed {
        conversation,
        author,
        from: SequenceNumber::new(from).expect("a non-zero sequence"),
        to: SequenceNumber::new(to).expect("a non-zero sequence"),
        cause: GapCloseCause::ToleranceElapsed,
    }
}

#[test]
fn a_recorded_gap_is_reported_for_its_own_conversation() {
    let ledger = GapLedger::new();
    let broadcast = gap(ConversationId::Broadcast, alice(), 2, 4);
    let direct = gap(ConversationId::Direct(bob()), bob(), 5, 5);

    ledger.record(broadcast);
    ledger.record(direct);

    assert_eq!(ledger.of(ConversationId::Broadcast), vec![broadcast]);
    assert_eq!(ledger.of(ConversationId::Direct(bob())), vec![direct]);
}

#[test]
fn a_conversation_with_no_gaps_reports_none() {
    let ledger = GapLedger::new();

    assert!(ledger.of(ConversationId::Broadcast).is_empty());
}

#[test]
fn gaps_are_reported_oldest_first() {
    let ledger = GapLedger::new();
    let first = gap(ConversationId::Broadcast, alice(), 2, 2);
    let second = gap(ConversationId::Broadcast, alice(), 7, 9);

    ledger.record(first);
    ledger.record(second);

    assert_eq!(ledger.all(), vec![first, second]);
}

#[test]
fn the_oldest_entry_is_discarded_past_the_cap() {
    let ledger = GapLedger::with_capacity(2);
    let first = gap(ConversationId::Broadcast, alice(), 2, 2);
    let second = gap(ConversationId::Broadcast, alice(), 4, 4);
    let third = gap(ConversationId::Broadcast, alice(), 6, 6);

    ledger.record(first);
    ledger.record(second);
    ledger.record(third);

    assert_eq!(ledger.all(), vec![second, third]);
}

#[test]
fn a_one_message_gap_spans_one() {
    // The range is inclusive, so `from == to` is one message and not zero —
    // the number the pane prints.
    assert_eq!(
        abandoned_span(&gap(ConversationId::Broadcast, alice(), 5, 5)),
        1
    );
}

#[test]
fn a_multi_message_gap_spans_its_inclusive_range() {
    assert_eq!(
        abandoned_span(&gap(ConversationId::Broadcast, alice(), 3, 7)),
        5
    );
}
