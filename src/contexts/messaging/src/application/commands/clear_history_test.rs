use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::application::commands::{ClearHistory, ClearHistoryHandler};
use crate::application::test_context::{NOW, TestContext, TestContextBuilder, broadcast_from};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, SequenceNumber};
use crate::ports::port_fakes::{InMemorySequenceCounter, UnavailableMessageLog};
use crate::ports::{
    ClearHistoryPort, ClearedHistory, MessageLogError, MessageLogPort, MessagingQueryPort,
    SequenceCounterPort,
};
use crate::test_peers;

fn clear(context: &TestContext) -> ClearedHistory {
    context
        .context
        .history()
        .clear_history()
        .expect("an in-memory log clears")
}

/// `alice`, holding two conversations and three messages: the broadcast
/// channel (one said, one heard from `carol`) and a direct with `bob`.
///
/// `carol` gets no conversation of her own — a broadcast she sent belongs to
/// the channel, not to a thread with her.
fn spoken_in() -> TestContext {
    let context = TestContextBuilder::for_local_peer(test_peers::alice()).build();

    context.publish_broadcast("everyone").expect("published");
    context
        .send_direct(test_peers::bob(), "just you")
        .expect("sent");
    context
        .accept(broadcast_from(test_peers::carol(), 1, "hello", NOW))
        .expect("accepted");

    context
}

// ---------------------------------------------------------------- what goes

#[test]
fn clearing_empties_every_conversation_the_process_holds() {
    let context = spoken_in();

    clear(&context);

    assert!(context.history(ConversationId::Broadcast).is_empty());
    assert!(
        context
            .history(ConversationId::Direct(test_peers::bob()))
            .is_empty()
    );
    assert_eq!(context.context.queries().conversations(), Ok(Vec::new()));
}

#[test]
fn clearing_empties_the_log_mirror_as_well_as_the_conversations() {
    let context = spoken_in();

    clear(&context);

    assert!(context.mirrored(ConversationId::Broadcast).is_empty());
    assert!(
        context
            .mirrored(ConversationId::Direct(test_peers::bob()))
            .is_empty()
    );
}

/// A message buffered behind a gap is not part of any conversation yet, and it
/// must not survive a clear that took the conversation it was waiting for.
#[test]
fn clearing_discards_messages_still_buffered_behind_a_gap() {
    let context = TestContextBuilder::for_local_peer(test_peers::alice()).build();
    // Sequence 2 with no sequence 1: held, not applied.
    context
        .accept(broadcast_from(test_peers::carol(), 2, "second", NOW))
        .expect("accepted");

    clear(&context);
    // The gap's other half arrives after the clear. If the buffered message
    // had survived, both would appear.
    context
        .accept(broadcast_from(test_peers::carol(), 1, "first", NOW))
        .expect("accepted");

    assert_eq!(context.visible_text(ConversationId::Broadcast), ["first"]);
}

#[test]
fn clearing_reports_what_it_dropped() {
    let context = spoken_in();

    let cleared = clear(&context);

    assert_eq!(cleared.conversations_dropped, 2);
    assert_eq!(cleared.messages_dropped, 3);
    assert!(!cleared.is_empty());
}

#[test]
fn clearing_a_process_that_has_said_nothing_reports_nothing() {
    let context = TestContextBuilder::for_local_peer(test_peers::alice()).build();

    assert_eq!(clear(&context), ClearedHistory::default());
}

// ------------------------------------------------------------- what survives

/// The reason clearing is safe at all (D12, AC16).
///
/// The registry is dropped whole, high-water mark and all — and the mark comes
/// straight back, because a reopened conversation rehydrates from the counter
/// rather than from anything the clear touched. A peer that was already
/// listening still hears this one afterwards, because its numbers keep going
/// up.
#[test]
fn a_message_sent_after_a_clear_continues_the_sequence_rather_than_restarting_it() {
    let context = TestContextBuilder::for_local_peer(test_peers::alice()).build();
    context.publish_broadcast("one").expect("published");
    context.publish_broadcast("two").expect("published");

    clear(&context);
    context.publish_broadcast("three").expect("published");

    let after = context.history(ConversationId::Broadcast);
    assert_eq!(after.len(), 1, "only the message sent after the clear");
    assert_eq!(
        after[0].id().sequence(),
        SequenceNumber::new(3).expect("positive"),
        "the third number this identity has issued, not the first again — a \
         peer still holding the old mark would classify a restart as a duplicate"
    );
}

/// The same claim from the other side: the counter is never asked to go
/// backwards, because nothing on the clear path asks it anything at all.
#[test]
fn clearing_leaves_the_counter_exactly_where_it_was() {
    let counter = Arc::new(InMemorySequenceCounter::restored_with([(
        ConversationId::Broadcast,
        SequenceNumber::new(7).expect("positive"),
    )]));
    let context = TestContextBuilder::for_local_peer(test_peers::alice())
        .with_counter(Arc::clone(&counter) as Arc<dyn SequenceCounterPort + Send + Sync>)
        .build();
    context.publish_broadcast("eight").expect("published");

    clear(&context);

    assert_eq!(
        counter
            .last_issued(ConversationId::Broadcast)
            .expect("the fake answers"),
        Some(SequenceNumber::new(8).expect("positive"))
    );
}

/// Nothing outside this process may learn that a user cleared their screen —
/// and nothing that went is reported as loss, because a cleared log holds no
/// record of having been mid-stream (D10).
#[test]
fn clearing_publishes_no_event_at_all() {
    let context = spoken_in();
    let before = context.events().len();

    clear(&context);

    assert_eq!(context.events().len(), before);
    assert!(
        !context
            .events()
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageGapClosed(_)))
    );
}

// ------------------------------------------------------------------ failure

/// The registry cannot refuse and the log can. Doing the fallible half second
/// means a refusal leaves the user with less history than they had, never with
/// a screen that disagrees with the mirror behind it.
///
/// Assembled by hand rather than through the builder: the substitution is a
/// log that fails, and the builder's log is the one every other test asserts
/// against.
#[test]
fn a_log_that_refuses_still_leaves_the_conversations_cleared() {
    let registry = Arc::new(ConversationRegistry::for_local_peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::restored_with([])),
    ));
    let handler = ClearHistoryHandler::new(
        Arc::clone(&registry),
        Arc::new(UnavailableMessageLog) as Arc<dyn MessageLogPort + Send + Sync>,
    );

    let refusal = handler.handle(ClearHistory);

    assert_eq!(refusal, Err(MessageLogError::Unavailable));
    assert_eq!(registry.open_conversations(), Vec::new());
}
