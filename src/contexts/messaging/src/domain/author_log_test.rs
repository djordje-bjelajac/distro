use crate::domain::author_log::GapClose;
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
fn first_sight_establishes_the_origin_and_abandons_nothing() {
    // D10/A1: this log has committed to nothing, so 1..=4 were never in flight
    // to this peer (AC10). The stream starts at 5.
    let mut log = log();
    log.buffer(message(5), ARRIVED_AT);
    log.buffer(message(6), ARRIVED_AT);

    let close = log.close_gap();

    assert_eq!(close, GapClose::OriginEstablished { origin: seq(5) });
    assert_eq!(log.origin(), Some(seq(5)));
    assert_eq!(log.high_water_mark(), Some(seq(6)), "5 and 6 drained");
    assert_eq!(log.buffered_count(), 0);
}

#[test]
fn abandoning_a_gap_between_observed_sequences_moves_the_mark_and_drains_the_rest() {
    // A2: 1 and 5 were both observed here, so 2..=4 genuinely were in flight
    // and did not arrive. That is loss, and the range is named.
    let mut log = applied([1]);
    log.buffer(message(5), ARRIVED_AT);
    log.buffer(message(6), ARRIVED_AT);

    let close = log.close_gap();

    assert_eq!(
        close,
        GapClose::Abandoned {
            from: seq(2),
            to: seq(4),
        }
    );
    assert_eq!(log.origin(), Some(SequenceNumber::FIRST), "unmoved");
    assert_eq!(log.high_water_mark(), Some(seq(6)), "5 and 6 drained");
    assert_eq!(log.buffered_count(), 0);
}

#[test]
fn everything_inside_an_abandoned_range_is_out_of_reach_and_none_of_it_is_applied() {
    // Rule R.3, and the distinction invariant 6 turns on: a number below the
    // mark that this log does not hold is loss, not a duplicate.
    let mut log = applied([1]);
    log.buffer(message(5), ARRIVED_AT);

    assert_eq!(
        log.close_gap(),
        GapClose::Abandoned {
            from: seq(2),
            to: seq(4),
        }
    );
    for sequence in 2..=4 {
        assert!(!log.is_applied(seq(sequence)), "#{sequence}");
        assert!(log.is_out_of_reach(seq(sequence)), "#{sequence}");
    }
    assert!(log.is_applied(seq(5)));
    assert!(!log.is_out_of_reach(seq(5)));
    assert!(!log.is_out_of_reach(seq(6)));
}

#[test]
fn everything_below_an_origin_established_by_first_sight_is_out_of_reach_too() {
    // Nothing below the origin is *reported* as loss, but nothing below it is
    // admissible either: it is out of reach, never a duplicate (invariant 6).
    let mut log = log();
    log.buffer(message(5), ARRIVED_AT);

    assert_eq!(
        log.close_gap(),
        GapClose::OriginEstablished { origin: seq(5) }
    );
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

    assert_eq!(log.close_gap(), GapClose::Nothing);
    assert_eq!(log.high_water_mark(), Some(SequenceNumber::FIRST));
}

#[test]
fn a_rehydrated_log_meeting_its_author_again_reports_the_run_it_missed() {
    // A2 for the restart case: this log *did* issue up to 7, so 8 was in flight
    // to it and did not arrive. The rehydrated mark is an observation, so the
    // first-sight rule does not apply.
    let mut log = AuthorLog::rehydrated(test_peers::bob(), seq(7));
    log.buffer(message(9), ARRIVED_AT);

    assert_eq!(
        log.close_gap(),
        GapClose::Abandoned {
            from: seq(8),
            to: seq(8),
        }
    );
    assert_eq!(log.high_water_mark(), Some(seq(9)));
}

#[test]
fn first_sight_of_the_first_sequence_needs_no_close_at_all() {
    // Genesis is provable from the number itself, so it never reaches the
    // buffer and the origin is committed by applying it.
    let mut log = log();
    log.apply(message(1));

    assert_eq!(log.origin(), Some(SequenceNumber::FIRST));
    assert_eq!(log.close_gap(), GapClose::Nothing);
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
