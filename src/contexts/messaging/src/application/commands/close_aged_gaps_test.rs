use crate::application::MessagingSettings;
use crate::application::test_context::{TestContext, TestContextBuilder, broadcast_from};
use crate::domain::events::{GapCloseCause, MessagingEvent};
use crate::domain::{ConversationId, DurationMillis, Millis, SequenceNumber};
use crate::ports::InboundEnvelopePort;
use crate::test_peers;

const TOLERANCE_MILLIS: u64 = 2_000;
const CLAIMED_AT: Millis = Millis::from_millis(42);

fn alice() -> TestContext {
    TestContextBuilder::for_local_peer(test_peers::alice())
        .with_settings(
            MessagingSettings::for_local_peer(test_peers::alice())
                .with_gap_tolerance(DurationMillis::from_millis(TOLERANCE_MILLIS)),
        )
        .build()
}

fn sweep(context: &TestContext) -> Vec<crate::domain::events::MessageGapClosed> {
    context.context.inbound().close_aged_gaps().expect("sweep")
}

fn seq(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("non-zero")
}

#[test]
fn a_sweep_with_nothing_waiting_reports_nothing() {
    let context = alice();

    assert_eq!(sweep(&context), Vec::new());
    assert_eq!(context.events(), Vec::new());
}

#[test]
fn a_gap_younger_than_the_window_is_left_alone() {
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 3, "third", CLAIMED_AT))
        .expect("buffered");

    context.clock.advance(TOLERANCE_MILLIS - 1);

    assert_eq!(sweep(&context), Vec::new());
    assert_eq!(context.history(ConversationId::Broadcast), Vec::new());
}

#[test]
fn an_aged_gap_is_abandoned_and_the_range_is_named() {
    // AC15: an abandoned gap is visible and counted, never a silent skip.
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 3, "third", CLAIMED_AT))
        .expect("buffered");

    context.clock.advance(TOLERANCE_MILLIS);
    let closed = sweep(&context);

    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].conversation, ConversationId::Broadcast);
    assert_eq!(closed[0].author, test_peers::bob());
    assert_eq!(closed[0].from, seq(1));
    assert_eq!(closed[0].to, seq(2));
    assert_eq!(closed[0].cause, GapCloseCause::ToleranceElapsed);
}

#[test]
fn closing_a_gap_makes_the_run_behind_it_visible() {
    // AC10's affirmative half: a late joiner hears everything an author sends
    // after it joined, within one tolerance window of first contact. The sweep
    // is what delivers that.
    let context = alice();
    for (sequence, text) in [(3u64, "third"), (4, "fourth"), (5, "fifth")] {
        context
            .accept(broadcast_from(
                test_peers::bob(),
                sequence,
                text,
                CLAIMED_AT,
            ))
            .expect("buffered");
    }
    assert_eq!(
        context.history(ConversationId::Broadcast),
        Vec::new(),
        "nothing is visible while the gap is open"
    );

    context.clock.advance(TOLERANCE_MILLIS);
    sweep(&context);

    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["third", "fourth", "fifth"]
    );
    assert_eq!(
        context.mirrored(ConversationId::Broadcast).len(),
        3,
        "the released run is mirrored, not only announced"
    );
}

#[test]
fn the_abandoned_range_is_announced_before_what_it_released() {
    // The gap event explains the jump in the messages that follow it; the other
    // order would make a consumer reason backwards from a hole it had drawn.
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 3, "third", CLAIMED_AT))
        .expect("buffered");
    context.clock.advance(TOLERANCE_MILLIS);

    sweep(&context);

    let events = context.events();
    assert!(matches!(events[0], MessagingEvent::MessageGapClosed(_)));
    assert!(matches!(events[1], MessagingEvent::MessageReceived(_)));
    assert_eq!(events.len(), 2);
}

#[test]
fn a_message_arriving_after_its_gap_closed_is_reported_and_not_shown() {
    // AC8's amended clause and invariant 6 as tightened: this is loss, not a
    // duplicate, and calling it a duplicate would hide it.
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 3, "third", CLAIMED_AT))
        .expect("buffered");
    context.clock.advance(TOLERANCE_MILLIS);
    sweep(&context);

    let late = context
        .accept(broadcast_from(test_peers::bob(), 2, "second", CLAIMED_AT))
        .expect("judged");

    assert!(late.is_refused());
    assert!(!late.is_duplicate());
    assert_eq!(
        late.rejection_reason(),
        Some(crate::domain::events::RejectionReason::ArrivedAfterGapClosed)
    );
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["third"]
    );
}

#[test]
fn a_second_sweep_over_a_settled_conversation_does_nothing() {
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 3, "third", CLAIMED_AT))
        .expect("buffered");
    context.clock.advance(TOLERANCE_MILLIS);
    sweep(&context);
    let after_first = context.events().len();

    context.clock.advance(TOLERANCE_MILLIS * 10);

    assert_eq!(sweep(&context), Vec::new());
    assert_eq!(context.events().len(), after_first);
}

#[test]
fn each_authors_gap_is_judged_on_its_own_arrival() {
    // Sequence numbers are per (author, conversation) and never interact: one
    // author's silence must not close another's gap.
    let context = alice();
    context
        .accept(broadcast_from(
            test_peers::bob(),
            3,
            "bob third",
            CLAIMED_AT,
        ))
        .expect("buffered");
    context.clock.advance(TOLERANCE_MILLIS);
    context
        .accept(broadcast_from(
            test_peers::carol(),
            3,
            "carol third",
            CLAIMED_AT,
        ))
        .expect("buffered");

    let closed = sweep(&context);

    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].author, test_peers::bob());
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["bob third"]
    );
}

#[test]
fn a_direct_conversations_gap_closes_by_the_same_rule() {
    // Rule R is identical for `Broadcast` and `Direct`.
    let context = alice();
    let conversation = ConversationId::Direct(test_peers::bob());
    context
        .accept(crate::application::test_context::direct_from(
            test_peers::bob(),
            4,
            "fourth",
            CLAIMED_AT,
        ))
        .expect("buffered");

    context.clock.advance(TOLERANCE_MILLIS);
    let closed = sweep(&context);

    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].conversation, conversation);
    assert_eq!(closed[0].from, seq(1));
    assert_eq!(closed[0].to, seq(3));
    assert_eq!(context.visible_text(conversation), vec!["fourth"]);
}

#[test]
fn a_gap_that_fills_in_time_closes_with_no_diagnostic_at_all() {
    // Ordinary reordering must not raise a false alarm: the messages arrive
    // inside the window, the run completes, and nothing is abandoned.
    let context = alice();
    context
        .accept(broadcast_from(test_peers::bob(), 2, "second", CLAIMED_AT))
        .expect("buffered");
    context.clock.advance(TOLERANCE_MILLIS / 4);
    context
        .accept(broadcast_from(test_peers::bob(), 1, "first", CLAIMED_AT))
        .expect("applied");

    context.clock.advance(TOLERANCE_MILLIS);

    assert_eq!(sweep(&context), Vec::new());
    assert_eq!(
        context.visible_text(ConversationId::Broadcast),
        vec!["first", "second"]
    );
    assert!(
        !context
            .events()
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageGapClosed(_))),
        "no gap was ever abandoned"
    );
}
