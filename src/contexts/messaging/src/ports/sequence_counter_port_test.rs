use crate::domain::{ConversationId, SequenceNumber};
use crate::ports::port_fakes::{InMemorySequenceCounter, UnavailableSequenceCounter};
use crate::ports::{SequenceCounterError, SequenceCounterPort};
use crate::test_peers;

fn seq(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("positive")
}

#[test]
fn the_port_is_object_safe_so_one_counter_can_be_shared() {
    let counter = InMemorySequenceCounter::default();
    let port: &dyn SequenceCounterPort = &counter;

    assert_eq!(
        port.issue_next(ConversationId::Broadcast),
        Ok(SequenceNumber::FIRST)
    );
}

#[test]
fn a_conversation_nobody_has_spoken_in_has_issued_nothing() {
    let counter = InMemorySequenceCounter::default();

    assert_eq!(counter.last_issued(ConversationId::Broadcast), Ok(None));
}

#[test]
fn issuing_advances_the_counter_and_never_repeats_a_number() {
    let counter = InMemorySequenceCounter::default();

    for expected in 1..=5u64 {
        assert_eq!(
            counter.issue_next(ConversationId::Broadcast),
            Ok(seq(expected))
        );
        assert_eq!(
            counter.last_issued(ConversationId::Broadcast),
            Ok(Some(seq(expected))),
            "the advance is recorded before the number is used"
        );
    }
}

#[test]
fn each_conversation_counts_independently() {
    // Sequence numbers are per `(author, conversation)`, so the broadcast
    // channel and each direct conversation keep separate counters.
    let counter = InMemorySequenceCounter::default();
    let direct = ConversationId::Direct(test_peers::bob());

    counter.issue_next(ConversationId::Broadcast).expect("room");
    counter.issue_next(ConversationId::Broadcast).expect("room");

    assert_eq!(counter.issue_next(direct), Ok(SequenceNumber::FIRST));
    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Ok(Some(seq(2)))
    );
}

#[test]
fn a_counter_restored_from_its_store_resumes_where_it_left_off() {
    // D12/AC16: the counter shares the keypair's lifetime. A peer that restarts
    // must not re-issue numbers its listeners already hold, or every message it
    // sends is classified a duplicate and it goes silently mute.
    let counter = InMemorySequenceCounter::restored_with([(ConversationId::Broadcast, seq(7))]);

    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Ok(Some(seq(7)))
    );
    assert_eq!(counter.issue_next(ConversationId::Broadcast), Ok(seq(8)));
}

#[test]
fn an_exhausted_conversation_reports_it_rather_than_wrapping() {
    let counter =
        InMemorySequenceCounter::restored_with([(ConversationId::Broadcast, SequenceNumber::MAX)]);

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Exhausted)
    );
}

#[test]
fn an_unavailable_counter_reports_a_typed_error_on_every_operation() {
    let counter = UnavailableSequenceCounter;

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Unavailable)
    );
    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Err(SequenceCounterError::Unavailable)
    );
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        SequenceCounterError::Unavailable.to_string(),
        "the sequence counter is not available"
    );
    assert_eq!(
        SequenceCounterError::NotPersisted.to_string(),
        "the advanced sequence counter could not be recorded"
    );
    assert_eq!(
        SequenceCounterError::Exhausted.to_string(),
        "this conversation has no sequence number left"
    );
    assert_eq!(
        SequenceCounterError::UnsupportedSchemaVersion { found: 9 }.to_string(),
        "the sequence counter store has unsupported schema version 9"
    );
}
