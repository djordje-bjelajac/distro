use crate::domain::{ConversationId, MessageId, SequenceNumber};
use crate::test_peers;

fn seq(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("positive")
}

#[test]
fn an_identifier_carries_author_conversation_and_sequence() {
    let id = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(3));

    assert_eq!(id.author(), test_peers::bob());
    assert_eq!(id.conversation(), ConversationId::Broadcast);
    assert_eq!(id.sequence(), seq(3));
}

#[test]
fn the_same_sequence_from_different_authors_is_a_different_message() {
    // Sequences are per (author, conversation), so the author is part of
    // identity — dedup would collapse unrelated messages otherwise.
    let alice = MessageId::new(test_peers::alice(), ConversationId::Broadcast, seq(1));
    let bob = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(1));

    assert_ne!(alice, bob);
}

#[test]
fn the_same_sequence_in_different_conversations_is_a_different_message() {
    let broadcast = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(1));
    let direct = MessageId::new(
        test_peers::bob(),
        ConversationId::Direct(test_peers::alice()),
        seq(1),
    );

    assert_ne!(broadcast, direct);
}

#[test]
fn identifiers_with_the_same_three_parts_are_equal() {
    let id = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(7));
    let same = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(7));

    assert_eq!(id, same);
}
