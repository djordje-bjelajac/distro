use messaging::domain::{ConversationId, Message, MessageBody, MessageId, Millis, SequenceNumber};
use messaging::ports::{MessageLogError, MessageLogPort};
use shared_types::PeerId;

use crate::crypto::SimKeypair;
use crate::stores::InMemoryMessageLog;

fn alice() -> PeerId {
    SimKeypair::derived(1, "alice").peer()
}

fn bob() -> PeerId {
    SimKeypair::derived(1, "bob").peer()
}

fn message(conversation: ConversationId, sequence: u64, text: &str) -> Message {
    Message::received(
        MessageId::new(
            alice(),
            conversation,
            SequenceNumber::new(sequence).expect("non-zero"),
        ),
        MessageBody::new(text).expect("a valid body"),
        Millis::from_millis(1),
    )
}

#[test]
fn a_conversation_nobody_has_spoken_in_loads_as_empty() {
    let log = InMemoryMessageLog::default();

    assert_eq!(log.load(ConversationId::Broadcast), Ok(Vec::new()));
    assert!(log.is_empty());
}

#[test]
fn messages_come_back_in_append_order() {
    let log = InMemoryMessageLog::default();

    for sequence in 1..=3 {
        log.append(&message(ConversationId::Broadcast, sequence, "hi"))
            .expect("room in the log");
    }

    let sequences: Vec<u64> = log
        .load(ConversationId::Broadcast)
        .expect("healthy log")
        .iter()
        .map(|message| message.sequence().as_u64())
        .collect();

    assert_eq!(sequences, vec![1, 2, 3]);
}

#[test]
fn conversations_are_listed_deterministically_with_broadcast_first() {
    let log = InMemoryMessageLog::default();

    log.append(&message(ConversationId::Direct(bob()), 1, "dm"))
        .expect("room in the log");
    log.append(&message(ConversationId::Broadcast, 1, "all"))
        .expect("room in the log");

    assert_eq!(
        log.conversations(),
        Ok(vec![
            ConversationId::Broadcast,
            ConversationId::Direct(bob())
        ])
    );
}

#[test]
fn reaching_the_cap_is_a_stated_refusal_rather_than_a_quiet_eviction() {
    // S6 bounds in-memory history; AC11/AC15 make silent loss a non-state, so
    // the bound is reported instead of dropping the oldest thing anyone said.
    let log = InMemoryMessageLog::with_capacity(2);

    log.append(&message(ConversationId::Broadcast, 1, "one"))
        .expect("room in the log");
    log.append(&message(ConversationId::Broadcast, 2, "two"))
        .expect("room in the log");

    assert_eq!(
        log.append(&message(ConversationId::Broadcast, 3, "three")),
        Err(MessageLogError::CapacityExhausted)
    );
    assert_eq!(log.len(), 2);
}

#[test]
fn the_cap_counts_every_conversation_together() {
    let log = InMemoryMessageLog::with_capacity(1);

    log.append(&message(ConversationId::Broadcast, 1, "all"))
        .expect("room in the log");

    assert_eq!(
        log.append(&message(ConversationId::Direct(bob()), 1, "dm")),
        Err(MessageLogError::CapacityExhausted)
    );
}
