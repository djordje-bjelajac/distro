use messaging::domain::{ConversationId, Message, MessageBody, MessageId, Millis, SequenceNumber};
use messaging::ports::{MessageLogError, MessageLogPort};
use shared_types::PeerId;

use crate::stores::InMemoryMessageLog;
use crate::test_peers::{alice, bob};

fn message(author: PeerId, conversation: ConversationId, sequence: u64, text: &str) -> Message {
    Message::received(
        MessageId::new(
            author,
            conversation,
            SequenceNumber::new(sequence).expect("a sequence number starts at 1"),
        ),
        MessageBody::new(text).expect("a valid fixture body"),
        Millis::from_millis(sequence),
    )
}

#[test]
fn a_new_log_holds_nothing() {
    let log = InMemoryMessageLog::default();

    assert!(log.is_empty());
    assert_eq!(log.conversations(), Ok(Vec::new()));
}

#[test]
fn an_unspoken_conversation_loads_empty_rather_than_failing() {
    let log = InMemoryMessageLog::default();

    // Absence of history is not an error.
    assert_eq!(log.load(ConversationId::Broadcast), Ok(Vec::new()));
}

#[test]
fn messages_come_back_in_append_order() {
    let log = InMemoryMessageLog::default();
    let first = message(alice(), ConversationId::Broadcast, 1, "first");
    let second = message(alice(), ConversationId::Broadcast, 2, "second");

    log.append(&first).expect("the append must land");
    log.append(&second).expect("the append must land");

    assert_eq!(log.load(ConversationId::Broadcast), Ok(vec![first, second]));
    assert_eq!(log.len(), 2);
}

#[test]
fn conversations_are_kept_apart() {
    let log = InMemoryMessageLog::default();
    let broadcast = message(alice(), ConversationId::Broadcast, 1, "to everyone");
    let direct = message(alice(), ConversationId::Direct(bob()), 1, "to bob");

    log.append(&broadcast).expect("the append must land");
    log.append(&direct).expect("the append must land");

    assert_eq!(log.load(ConversationId::Broadcast), Ok(vec![broadcast]));
    assert_eq!(log.load(ConversationId::Direct(bob())), Ok(vec![direct]));
}

#[test]
fn conversations_are_listed_in_a_deterministic_order() {
    let log = InMemoryMessageLog::default();

    log.append(&message(
        alice(),
        ConversationId::Direct(bob()),
        1,
        "to bob",
    ))
    .expect("the append must land");
    log.append(&message(
        alice(),
        ConversationId::Broadcast,
        1,
        "to everyone",
    ))
    .expect("the append must land");

    // Broadcast first, then directs by peer id — the order `ConversationId`
    // itself sorts in (AC13).
    assert_eq!(
        log.conversations(),
        Ok(vec![
            ConversationId::Broadcast,
            ConversationId::Direct(bob())
        ])
    );
}

#[test]
fn a_full_log_says_so_rather_than_evicting_the_oldest_thing_anyone_said() {
    let log = InMemoryMessageLog::with_capacity(2);

    log.append(&message(alice(), ConversationId::Broadcast, 1, "first"))
        .expect("the append must land");
    log.append(&message(alice(), ConversationId::Broadcast, 2, "second"))
        .expect("the append must land");

    assert_eq!(
        log.append(&message(alice(), ConversationId::Broadcast, 3, "third")),
        Err(MessageLogError::CapacityExhausted)
    );
    // Silently dropping content is the loss AC11 and AC15 rule out; the two
    // that fit are still there.
    assert_eq!(log.len(), 2);
}

#[test]
fn the_capacity_counts_across_conversations() {
    let log = InMemoryMessageLog::with_capacity(1);

    log.append(&message(alice(), ConversationId::Broadcast, 1, "first"))
        .expect("the append must land");

    assert_eq!(
        log.append(&message(
            alice(),
            ConversationId::Direct(bob()),
            1,
            "second"
        )),
        Err(MessageLogError::CapacityExhausted)
    );
}

#[test]
fn history_does_not_outlive_the_log() {
    let first = InMemoryMessageLog::default();
    first
        .append(&message(alice(), ConversationId::Broadcast, 1, "said once"))
        .expect("the append must land");
    drop(first);

    // D7 stated as a test: a rebuilt peer starts with nothing, which is the
    // condition that makes the persisted sequence counter necessary (D12).
    assert!(InMemoryMessageLog::default().is_empty());
}
