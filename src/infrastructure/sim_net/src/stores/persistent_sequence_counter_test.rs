use messaging::domain::{ConversationId, SequenceNumber};
use messaging::ports::{SequenceCounterError, SequenceCounterPort};

use crate::crypto::SimKeypair;
use crate::stores::PersistentSequenceCounter;

fn bob() -> shared_types::PeerId {
    SimKeypair::derived(1, "bob").peer()
}

#[test]
fn a_fresh_counter_has_issued_nothing() {
    let counter = PersistentSequenceCounter::fresh();

    assert_eq!(counter.last_issued(ConversationId::Broadcast), Ok(None));
}

#[test]
fn the_first_number_issued_is_one() {
    let counter = PersistentSequenceCounter::fresh();

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Ok(SequenceNumber::FIRST)
    );
}

#[test]
fn numbers_are_strictly_monotonic_within_a_conversation() {
    let counter = PersistentSequenceCounter::fresh();

    let issued: Vec<u64> = (0..5)
        .map(|_| {
            counter
                .issue_next(ConversationId::Broadcast)
                .expect("counter is healthy")
                .as_u64()
        })
        .collect();

    assert_eq!(issued, vec![1, 2, 3, 4, 5]);
}

#[test]
fn each_conversation_counts_independently() {
    let counter = PersistentSequenceCounter::fresh();

    counter
        .issue_next(ConversationId::Broadcast)
        .expect("counter is healthy");
    counter
        .issue_next(ConversationId::Broadcast)
        .expect("counter is healthy");

    assert_eq!(
        counter.issue_next(ConversationId::Direct(bob())),
        Ok(SequenceNumber::FIRST)
    );
    assert_eq!(
        counter.last_issued(ConversationId::Broadcast),
        Ok(SequenceNumber::new(2).ok())
    );
}

#[test]
fn a_counter_that_survived_a_restart_resumes_where_it_stopped() {
    // D12/AC16 at the store level: the counter's domain of validity is the
    // identity, not the process.
    let counter = PersistentSequenceCounter::resuming_at(
        ConversationId::Broadcast,
        SequenceNumber::new(9).expect("nine is a sequence number"),
    );

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        SequenceNumber::new(10).map_err(|_| SequenceCounterError::Exhausted)
    );
}

#[test]
fn the_advance_is_recorded_before_it_is_returned() {
    let counter = PersistentSequenceCounter::fresh();

    let issued = counter
        .issue_next(ConversationId::Broadcast)
        .expect("counter is healthy");

    assert_eq!(counter.mark(ConversationId::Broadcast), Some(issued));
}

#[test]
fn an_injected_fault_issues_nothing_and_records_nothing() {
    // The port's contract: reporting NotPersisted and sending nothing is
    // strictly better than sending something every listener will ignore.
    let counter = PersistentSequenceCounter::fresh();
    counter.fail_with(SequenceCounterError::NotPersisted);

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::NotPersisted)
    );
    assert_eq!(counter.mark(ConversationId::Broadcast), None);

    counter.repair();
    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Ok(SequenceNumber::FIRST)
    );
}

#[test]
fn an_exhausted_conversation_refuses_rather_than_wrapping() {
    // Wrapping would re-issue numbers an author already used, which every
    // dedup and ordering rule in messaging assumes never happens.
    let counter =
        PersistentSequenceCounter::resuming_at(ConversationId::Broadcast, SequenceNumber::MAX);

    assert_eq!(
        counter.issue_next(ConversationId::Broadcast),
        Err(SequenceCounterError::Exhausted)
    );
}

#[test]
fn conversations_are_listed_in_a_deterministic_order() {
    let counter = PersistentSequenceCounter::fresh();

    counter
        .issue_next(ConversationId::Direct(bob()))
        .expect("counter is healthy");
    counter
        .issue_next(ConversationId::Broadcast)
        .expect("counter is healthy");

    assert_eq!(
        counter.conversations(),
        vec![ConversationId::Broadcast, ConversationId::Direct(bob())]
    );
}
