use crate::domain::{
    AuthorLog, ConversationId, Message, MessageBody, MessageId, Millis, SequenceNumber,
};
use crate::test_peers;

const SENT_AT: Millis = Millis::from_millis(500);
const ARRIVED_AT: Millis = Millis::from_millis(10_000);

fn seq(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("positive")
}

fn message(sequence: u64) -> Message {
    Message::received(
        MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(sequence)),
        MessageBody::new("text").expect("valid body"),
        SENT_AT,
    )
}

fn log() -> AuthorLog {
    AuthorLog::empty(test_peers::bob())
}

/// Applies `sequences` in order, as the aggregate would once each is
/// contiguous.
fn applied(sequences: impl IntoIterator<Item = u64>) -> AuthorLog {
    let mut log = log();
    for sequence in sequences {
        log.apply(message(sequence));
    }
    log
}

#[test]
fn a_fresh_log_has_committed_to_nothing_and_puts_nothing_out_of_reach() {
    let log = log();

    assert_eq!(log.origin(), None);
    assert_eq!(log.high_water_mark(), None);
    assert!(!log.has_gap());
    for sequence in [1, 2, 99] {
        assert!(!log.is_applied(seq(sequence)));
        assert!(!log.is_out_of_reach(seq(sequence)));
    }
}

#[test]
fn the_first_applied_message_commits_the_origin() {
    let log = applied([1, 2, 3]);

    assert_eq!(log.origin(), Some(SequenceNumber::FIRST));
    assert_eq!(log.high_water_mark(), Some(seq(3)));
    for sequence in [1, 2, 3] {
        assert!(log.is_applied(seq(sequence)));
        assert!(!log.is_out_of_reach(seq(sequence)));
    }
    assert!(!log.is_out_of_reach(seq(4)), "4 has simply not arrived yet");
}

#[test]
fn abandoning_a_gap_moves_the_mark_below_what_was_held_and_drains_the_rest() {
    let mut log = log();
    log.buffer(message(5), ARRIVED_AT);
    log.buffer(message(6), ARRIVED_AT);

    let range = log.close_gap();

    assert_eq!(range, Some((SequenceNumber::FIRST, seq(4))));
    assert_eq!(log.origin(), Some(seq(4)));
    assert_eq!(log.high_water_mark(), Some(seq(6)), "5 and 6 drained");
    assert_eq!(log.buffered_count(), 0);
}

#[test]
fn everything_inside_an_abandoned_range_is_out_of_reach_and_none_of_it_is_applied() {
    // Rule R.3, and the distinction invariant 6 turns on: a number below the
    // mark that this log does not hold is loss, not a duplicate.
    let mut log = log();
    log.buffer(message(5), ARRIVED_AT);
    log.close_gap().expect("a gap to abandon");

    for sequence in 1..=4 {
        assert!(!log.is_applied(seq(sequence)), "#{sequence}");
        assert!(log.is_out_of_reach(seq(sequence)), "#{sequence}");
    }
    assert!(log.is_applied(seq(5)));
    assert!(!log.is_out_of_reach(seq(5)));
    assert!(!log.is_out_of_reach(seq(6)));
}

#[test]
fn a_rehydrated_log_holds_nothing_yet_admits_nothing_it_has_already_issued() {
    // D12: the mark says what this peer *issued* before it restarted; the
    // messages themselves are gone (D7).
    let log = AuthorLog::rehydrated(test_peers::bob(), seq(7));

    assert!(log.messages().is_empty());
    assert_eq!(log.origin(), Some(seq(7)));
    for sequence in 1..=7 {
        assert!(!log.is_applied(seq(sequence)), "#{sequence}");
        assert!(log.is_out_of_reach(seq(sequence)), "#{sequence}");
    }
    assert!(!log.is_out_of_reach(seq(8)));
}

#[test]
fn a_log_with_nothing_held_has_no_gap_to_abandon() {
    let mut log = applied([1]);

    assert_eq!(log.close_gap(), None);
    assert_eq!(log.high_water_mark(), Some(SequenceNumber::FIRST));
}

#[test]
fn the_buffer_refuses_to_grow_past_its_cap() {
    let mut log = log();
    for sequence in 2..=(AuthorLog::MAX_BUFFERED_MESSAGES as u64 + 1) {
        assert!(log.buffer(message(sequence), ARRIVED_AT));
    }

    assert!(log.is_buffer_full());
    assert!(
        !log.buffer(message(1_000), ARRIVED_AT),
        "nothing was stored"
    );
    assert_eq!(log.buffered_count(), AuthorLog::MAX_BUFFERED_MESSAGES);
}

#[test]
fn the_oldest_arrival_is_the_one_a_gap_ages_from() {
    let mut log = log();
    log.buffer(message(9), Millis::from_millis(900));
    log.buffer(message(4), Millis::from_millis(400));
    log.buffer(message(7), Millis::from_millis(700));

    assert_eq!(log.oldest_buffered_at(), Some(Millis::from_millis(400)));
    assert_eq!(log.lowest_buffered(), Some(seq(4)));
}
