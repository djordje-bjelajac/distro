use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::application::test_context::sequence;
use crate::domain::{Conversation, ConversationId, SequenceNumber};
use crate::ports::port_fakes::{InMemorySequenceCounter, UnavailableSequenceCounter};
use crate::ports::{MessagingCommandError, SequenceCounterPort};
use crate::test_peers;

fn registry_with(counter: Arc<dyn SequenceCounterPort + Send + Sync>) -> ConversationRegistry {
    ConversationRegistry::for_local_peer(test_peers::alice(), counter)
}

fn empty_registry() -> ConversationRegistry {
    registry_with(Arc::new(InMemorySequenceCounter::default()))
}

#[test]
fn a_conversation_is_opened_at_the_counters_mark_on_first_touch() {
    // D12/AC16. History dies with the process; the counter does not, and this
    // is where the two are reconciled — before any handler can forget to.
    let registry = registry_with(Arc::new(InMemorySequenceCounter::restored_with([(
        ConversationId::Broadcast,
        sequence(41),
    )])));

    let mark = registry
        .modify(ConversationId::Broadcast, |open| {
            open.high_water_mark(&test_peers::alice())
        })
        .expect("the counter answers");

    assert_eq!(mark, Some(sequence(41)));
}

#[test]
fn a_conversation_the_local_peer_has_never_spoken_in_opens_with_no_mark() {
    let registry = empty_registry();

    let mark = registry
        .modify(ConversationId::Broadcast, |open| {
            open.high_water_mark(&test_peers::alice())
        })
        .expect("opened");

    assert_eq!(mark, None);
}

#[test]
fn the_restored_mark_records_what_was_issued_and_not_what_is_held() {
    // `AuthorLog::is_applied` tests membership precisely so a restored mark is
    // never mistaken for content this peer holds (invariant 6, as tightened).
    let registry = registry_with(Arc::new(InMemorySequenceCounter::restored_with([(
        ConversationId::Broadcast,
        sequence(41),
    )])));

    let empty = registry
        .modify(ConversationId::Broadcast, |open| open.is_empty())
        .expect("opened");

    assert!(empty);
}

#[test]
fn touching_a_conversation_twice_opens_it_once() {
    let counter = Arc::new(InMemorySequenceCounter::default());
    let registry =
        registry_with(Arc::clone(&counter) as Arc<dyn SequenceCounterPort + Send + Sync>);

    registry
        .modify(ConversationId::Broadcast, |open| {
            open.append_local(
                crate::application::test_context::body("first"),
                crate::domain::Millis::ZERO,
            )
        })
        .expect("opened")
        .expect("appended");
    let count = registry
        .modify(ConversationId::Broadcast, |open| open.applied_len())
        .expect("already open");

    assert_eq!(count, 1, "the second touch found the first one's state");
    assert_eq!(
        registry.open_conversations(),
        vec![ConversationId::Broadcast]
    );
}

#[test]
fn reading_never_opens_a_conversation() {
    // What makes the query side genuinely read-only.
    let registry = empty_registry();

    for _ in 0..5 {
        assert_eq!(
            registry.read(ConversationId::Broadcast, Conversation::applied_len),
            None
        );
    }

    assert_eq!(registry.open_conversations(), Vec::new());
}

#[test]
fn modifying_only_an_open_conversation_never_opens_one() {
    let registry = empty_registry();

    let touched = registry.modify_open(ConversationId::Direct(test_peers::bob()), |_| ());

    assert_eq!(touched, None);
    assert_eq!(registry.open_conversations(), Vec::new());
}

#[test]
fn a_counter_that_cannot_be_reached_refuses_to_open_a_conversation() {
    // Opening one anyway would mean guessing the mark, and a wrong guess is the
    // permanently-mute failure D12 exists to prevent.
    let registry = registry_with(Arc::new(UnavailableSequenceCounter));

    let outcome = registry.modify(ConversationId::Broadcast, |_| ());

    assert!(matches!(outcome, Err(MessagingCommandError::Sequence(_))));
    assert_eq!(registry.open_conversations(), Vec::new());
}

#[test]
fn a_direct_conversation_with_the_local_peer_itself_is_refused() {
    let registry = empty_registry();

    let outcome = registry.modify(ConversationId::Direct(test_peers::alice()), |_| ());

    assert!(matches!(
        outcome,
        Err(MessagingCommandError::Conversation(
            crate::domain::ConversationError::SelfConversation
        ))
    ));
}

#[test]
fn a_sweep_visits_every_open_conversation_in_identifier_order() {
    let registry = empty_registry();
    for id in [
        ConversationId::Direct(test_peers::carol()),
        ConversationId::Broadcast,
        ConversationId::Direct(test_peers::bob()),
    ] {
        registry.modify(id, |_| ()).expect("opened");
    }

    let visited = registry.sweep(|open| open.id());

    let mut expected = vec![
        ConversationId::Broadcast,
        ConversationId::Direct(test_peers::bob()),
        ConversationId::Direct(test_peers::carol()),
    ];
    expected.sort_unstable();
    assert_eq!(visited, expected);
    assert_eq!(visited[0], ConversationId::Broadcast);
}

#[test]
fn the_registry_knows_which_peer_it_belongs_to() {
    assert_eq!(empty_registry().local_peer(), test_peers::alice());
}

#[test]
fn the_counters_first_number_and_the_aggregates_first_number_agree() {
    // The guard in `OutboundComposer::record` compares these two; if they could
    // disagree at genesis the guard would fire on every fresh conversation.
    let counter = InMemorySequenceCounter::default();

    assert_eq!(
        counter
            .issue_next(ConversationId::Broadcast)
            .expect("issued"),
        SequenceNumber::FIRST
    );
    assert_eq!(
        SequenceNumber::following(None).expect("genesis"),
        SequenceNumber::FIRST
    );
}
