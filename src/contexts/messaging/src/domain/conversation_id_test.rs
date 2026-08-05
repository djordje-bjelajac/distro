use crate::domain::ConversationId;
use crate::test_peers;

#[test]
fn the_broadcast_channel_is_a_single_conversation() {
    let id = ConversationId::Broadcast;

    assert!(id.is_broadcast());
    assert!(!id.is_direct());
    assert_eq!(id.counterpart(), None);
    assert_eq!(ConversationId::Broadcast, ConversationId::Broadcast);
}

#[test]
fn a_direct_conversation_is_identified_by_its_counterpart() {
    let id = ConversationId::Direct(test_peers::bob());

    assert!(id.is_direct());
    assert!(!id.is_broadcast());
    assert_eq!(id.counterpart(), Some(test_peers::bob()));
}

#[test]
fn direct_conversations_with_different_peers_are_different_conversations() {
    assert_ne!(
        ConversationId::Direct(test_peers::bob()),
        ConversationId::Direct(test_peers::carol())
    );
    assert_ne!(
        ConversationId::Direct(test_peers::bob()),
        ConversationId::Broadcast
    );
}

#[test]
fn conversations_order_deterministically_so_listings_are_stable() {
    let broadcast = ConversationId::Broadcast;
    let bob = ConversationId::Direct(test_peers::bob());
    let carol = ConversationId::Direct(test_peers::carol());

    let mut one_way = [carol, broadcast, bob];
    one_way.sort();
    let mut another_way = [bob, carol, broadcast];
    another_way.sort();

    assert_eq!(one_way[0], broadcast, "the broadcast channel sorts first");
    assert_eq!(
        one_way, another_way,
        "the order is a property of the identifiers, not of how they arrived"
    );
}
