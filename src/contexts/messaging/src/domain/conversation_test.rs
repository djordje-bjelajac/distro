use shared_types::PeerId;

use crate::domain::events::{
    GapCloseCause, MessageDuplicateIgnored, MessageGapClosed, RejectionReason,
};
use crate::domain::{
    AuthorLog, Conversation, ConversationError, ConversationId, DeliveryFailure, DeliveryState,
    DurationMillis, InboundOutcome, MessageBody, MessageId, MessagePlacement, Millis,
    SequenceNumber,
};
use crate::test_peers;

/// The author's own claim about when it sent a message. Deliberately far from
/// [`ARRIVED_AT`]: nothing in these tests may let it drive local ageing.
const SENT_AT: Millis = Millis::from_millis(1_700_000_000_000);

/// The local instant every arrival in these tests lands at, unless it says
/// otherwise.
const ARRIVED_AT: Millis = Millis::from_millis(10_000);

const TOLERANCE: DurationMillis = Conversation::GAP_TOLERANCE;

fn at(millis: u64) -> Millis {
    Millis::from_millis(millis)
}

/// The first instant at which a gap opened at [`ARRIVED_AT`] may be abandoned.
fn tolerance_elapsed() -> Millis {
    at(ARRIVED_AT.as_millis() + TOLERANCE.as_millis())
}

fn body(text: &str) -> MessageBody {
    MessageBody::new(text).expect("test body is valid")
}

fn seq(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("positive")
}

/// A broadcast conversation local to `alice`.
fn broadcast() -> Conversation {
    Conversation::broadcast(test_peers::alice())
}

/// A direct conversation between `alice` (local) and `bob`.
fn direct() -> Conversation {
    Conversation::direct(test_peers::alice(), test_peers::bob()).expect("two distinct peers")
}

fn accept_at(
    conversation: &mut Conversation,
    author: PeerId,
    sequence: u64,
    text: &str,
    received_at: Millis,
) -> InboundOutcome {
    conversation
        .accept_remote(author, seq(sequence), body(text), SENT_AT, received_at)
        .expect("author belongs to the conversation")
}

fn accept(
    conversation: &mut Conversation,
    author: PeerId,
    sequence: u64,
    text: &str,
) -> InboundOutcome {
    accept_at(conversation, author, sequence, text, ARRIVED_AT)
}

/// The sequence numbers visible in the read view, for one author.
fn visible(conversation: &Conversation, author: PeerId) -> Vec<u64> {
    conversation
        .messages_by(&author)
        .iter()
        .map(|message| message.sequence().as_u64())
        .collect()
}

/// The sequence numbers one outcome made visible, in order.
fn became_visible(outcome: &InboundOutcome) -> Vec<u64> {
    outcome
        .applied()
        .iter()
        .map(|event| event.id.sequence().as_u64())
        .collect()
}

fn rejection_of(outcome: &InboundOutcome) -> RejectionReason {
    match outcome.placement() {
        MessagePlacement::Rejected(rejected) => rejected.reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

// ------------------------------------------------------------- construction

#[test]
fn a_new_broadcast_conversation_knows_only_which_peer_it_is_local_to() {
    let conversation = broadcast();

    assert_eq!(conversation.id(), ConversationId::Broadcast);
    assert_eq!(conversation.local_peer(), test_peers::alice());
    assert!(conversation.is_empty());
    assert_eq!(conversation.applied_len(), 0);
}

#[test]
fn a_direct_conversation_is_identified_by_its_counterpart() {
    let conversation = direct();

    assert_eq!(conversation.id(), ConversationId::Direct(test_peers::bob()));
}

#[test]
fn a_direct_conversation_with_oneself_is_rejected() {
    assert_eq!(
        Conversation::direct(test_peers::alice(), test_peers::alice()).unwrap_err(),
        ConversationError::SelfConversation
    );
}

// ---------------------------------------------------- the local outbound path

#[test]
fn the_first_local_message_takes_the_first_sequence_number() {
    let mut conversation = broadcast();

    let sent = conversation
        .append_local(body("hello"), SENT_AT)
        .expect("room to grow");

    assert_eq!(sent.id.author(), test_peers::alice());
    assert_eq!(sent.id.conversation(), ConversationId::Broadcast);
    assert_eq!(sent.id.sequence(), SequenceNumber::FIRST);
    assert_eq!(sent.claimed_sent_at, SENT_AT);
}

#[test]
fn successive_local_messages_are_strictly_monotonic() {
    let mut conversation = broadcast();

    for expected in 1..=5u64 {
        let sent = conversation
            .append_local(body("hello"), SENT_AT)
            .expect("room to grow");
        assert_eq!(sent.id.sequence(), seq(expected));
    }

    assert_eq!(visible(&conversation, test_peers::alice()), [1, 2, 3, 4, 5]);
    assert_eq!(
        conversation.high_water_mark(&test_peers::alice()),
        Some(seq(5))
    );
}

#[test]
fn a_local_direct_message_is_pending_and_a_local_broadcast_is_published() {
    let mut direct = direct();
    let sent = direct.append_local(body("hi"), SENT_AT).expect("room");
    assert_eq!(
        direct.message(&sent.id).expect("applied").delivery_state(),
        DeliveryState::Pending
    );

    let mut broadcast = broadcast();
    let sent = broadcast.append_local(body("hi"), SENT_AT).expect("room");
    assert_eq!(
        broadcast
            .message(&sent.id)
            .expect("applied")
            .delivery_state(),
        DeliveryState::Published
    );
}

// -------------------------------------------------------------- rehydration

#[test]
fn a_rehydrated_conversation_resumes_the_local_sequence_where_the_counter_left_it() {
    // D12: the outbound counter shares the keypair's lifetime, so a restarted
    // peer continues its run instead of resetting to 1 and being heard by
    // nobody (AC16).
    let mut conversation =
        Conversation::rehydrate(ConversationId::Broadcast, test_peers::alice(), Some(seq(7)))
            .expect("the broadcast channel is never a conversation with oneself");

    assert!(
        conversation.is_empty(),
        "history does not survive the process (D7); only the counter does"
    );
    assert_eq!(
        conversation.high_water_mark(&test_peers::alice()),
        Some(seq(7))
    );

    let sent = conversation
        .append_local(body("after a restart"), SENT_AT)
        .expect("room to grow");

    assert_eq!(sent.id.sequence(), seq(8));
}

#[test]
fn a_rehydrated_mark_is_not_mistaken_for_content_the_conversation_holds() {
    // Invariant 6 as tightened: "already applied" means actually applied. A
    // rehydrated mark describes numbers this peer has *issued*, not messages it
    // holds.
    let conversation =
        Conversation::rehydrate(ConversationId::Broadcast, test_peers::alice(), Some(seq(7)))
            .expect("valid");

    let issued = MessageId::new(
        test_peers::alice(),
        ConversationId::Broadcast,
        SequenceNumber::FIRST,
    );
    assert!(conversation.message(&issued).is_none());
    assert!(conversation.messages_by(&test_peers::alice()).is_empty());
}

#[test]
fn rehydrating_with_no_counter_yields_a_conversation_that_has_said_nothing() {
    assert_eq!(
        Conversation::rehydrate(ConversationId::Broadcast, test_peers::alice(), None)
            .expect("valid"),
        broadcast()
    );
}

#[test]
fn a_rehydrated_direct_conversation_with_oneself_is_rejected() {
    assert_eq!(
        Conversation::rehydrate(
            ConversationId::Direct(test_peers::alice()),
            test_peers::alice(),
            None,
        )
        .unwrap_err(),
        ConversationError::SelfConversation
    );
}

// ------------------------------------------------------------ in-order intake

#[test]
fn an_in_order_remote_message_applies_immediately() {
    let mut conversation = broadcast();

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "first");

    assert!(outcome.is_applied());
    assert_eq!(became_visible(&outcome), [1]);
    assert_eq!(visible(&conversation, test_peers::bob()), [1]);
    assert_eq!(
        conversation.high_water_mark(&test_peers::bob()),
        Some(SequenceNumber::FIRST)
    );
}

#[test]
fn a_received_direct_message_arrives_already_delivered() {
    let mut conversation = direct();

    accept(&mut conversation, test_peers::bob(), 1, "hi");

    let message = &conversation.messages_by(&test_peers::bob())[0];
    assert_eq!(message.delivery_state(), DeliveryState::Delivered);
    assert_eq!(message.body().as_str(), "hi");
}

#[test]
fn each_author_counts_in_their_own_sequence_space() {
    let mut conversation = broadcast();

    accept(&mut conversation, test_peers::bob(), 1, "bob one");
    accept(&mut conversation, test_peers::carol(), 1, "carol one");
    accept(&mut conversation, test_peers::bob(), 2, "bob two");

    assert_eq!(visible(&conversation, test_peers::bob()), [1, 2]);
    assert_eq!(visible(&conversation, test_peers::carol()), [1]);
    assert_eq!(conversation.applied_len(), 3);
}

// ------------------------------------------------------- gaps and reordering

#[test]
fn a_gap_buffers_the_message_and_keeps_it_out_of_the_read_view() {
    let mut conversation = broadcast();

    let outcome = accept(&mut conversation, test_peers::bob(), 3, "third");

    assert_eq!(
        *outcome.placement(),
        MessagePlacement::Buffered {
            id: MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(3)),
            awaiting: SequenceNumber::FIRST,
        }
    );
    assert!(visible(&conversation, test_peers::bob()).is_empty());
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 1);
    assert_eq!(conversation.high_water_mark(&test_peers::bob()), None);
}

#[test]
fn messages_become_visible_in_send_order_only_as_the_gaps_close() {
    // AC8: arrival order 3, 1, 2 must display as 1, 2, 3 — and nothing may be
    // shown before the run leading to it is complete.
    let mut conversation = broadcast();

    accept(&mut conversation, test_peers::bob(), 3, "third");
    assert!(visible(&conversation, test_peers::bob()).is_empty());

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "first");
    assert_eq!(became_visible(&outcome), [1], "3 is still not contiguous");
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 1);

    let outcome = accept(&mut conversation, test_peers::bob(), 2, "second");
    assert_eq!(
        became_visible(&outcome),
        [2, 3],
        "2 and the message it unblocked, in order"
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [1, 2, 3]);
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 0);
}

#[test]
fn a_gap_means_not_yet_received_never_lost() {
    // Invariant 5: the buffered content survives intact and appears verbatim
    // once it becomes contiguous.
    let mut conversation = broadcast();

    accept(&mut conversation, test_peers::bob(), 2, "the second thing");
    accept(&mut conversation, test_peers::bob(), 1, "the first thing");

    let bodies: Vec<&str> = conversation
        .messages_by(&test_peers::bob())
        .iter()
        .map(|message| message.body().as_str())
        .collect();
    assert_eq!(bodies, ["the first thing", "the second thing"]);
}

#[test]
fn a_long_run_of_buffered_messages_drains_in_one_go() {
    let mut conversation = broadcast();

    for sequence in (2..=10).rev() {
        accept(&mut conversation, test_peers::bob(), sequence, "later");
    }
    assert!(visible(&conversation, test_peers::bob()).is_empty());

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "first");

    assert_eq!(became_visible(&outcome), (1..=10).collect::<Vec<_>>());
    assert!(outcome.closed_gap().is_none());
}

// ------------------------------------------ D10: first sight sets the origin

/// A1 — the direct regression. Observed live: a conversation reporting
/// "5 messages from a11e 5897 were never received" for messages that were never
/// in flight to this peer.
#[test]
fn first_contact_at_a_high_sequence_reports_no_loss_and_the_stream_starts_there() {
    let mut conversation = broadcast();

    let outcome = accept(
        &mut conversation,
        test_peers::bob(),
        6,
        "the first thing heard",
    );
    assert!(outcome.is_buffered(), "6 still waits the window out");

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(
        closed,
        Vec::new(),
        "AC10 gives a late joiner no history, so 1..=5 were never in flight \
         here and calling them lost is a lie about the network"
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [6]);
    assert_eq!(
        conversation.high_water_mark(&test_peers::bob()),
        Some(seq(6))
    );

    for sequence in 7..=9 {
        let outcome = accept(
            &mut conversation,
            test_peers::bob(),
            sequence,
            "the next one",
        );
        assert!(outcome.is_applied(), "#{sequence}");
        assert!(outcome.closed_gap().is_none(), "#{sequence}");
    }
    assert_eq!(visible(&conversation, test_peers::bob()), [6, 7, 8, 9]);
}

/// A2 — the guard that stops A1 becoming "delete the warning". 6 and 9 were
/// both observed here, so 7 and 8 genuinely were in flight and did not arrive.
#[test]
fn a_gap_between_two_observed_sequences_is_still_reported_as_loss() {
    let mut conversation = broadcast();
    accept(
        &mut conversation,
        test_peers::bob(),
        6,
        "the first thing heard",
    );
    conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);
    assert_eq!(visible(&conversation, test_peers::bob()), [6]);

    let later = at(20_000);
    accept_at(&mut conversation, test_peers::bob(), 9, "much later", later);
    let closed =
        conversation.close_aged_gaps(at(later.as_millis() + TOLERANCE.as_millis()), TOLERANCE);

    assert_eq!(
        closed,
        vec![MessageGapClosed {
            conversation: ConversationId::Broadcast,
            author: test_peers::bob(),
            from: seq(7),
            to: seq(8),
            cause: GapCloseCause::ToleranceElapsed,
        }],
        "loss between two observed sequences is still named, with its range"
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [6, 9]);
}

/// A1 — reordering at first contact still resolves inside the window, so the
/// origin lands on the *lowest* sequence heard, not the first one to arrive.
#[test]
fn reordering_at_first_contact_applies_both_in_order_with_no_gap_event() {
    let mut conversation = broadcast();

    let seventh = accept(&mut conversation, test_peers::bob(), 7, "seventh");
    assert!(seventh.is_buffered());

    let sixth = accept_at(
        &mut conversation,
        test_peers::bob(),
        6,
        "sixth",
        at(ARRIVED_AT.as_millis() + 1),
    );
    assert!(sixth.is_buffered(), "6 is not provably contiguous either");
    assert!(sixth.closed_gap().is_none());

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(closed, Vec::new());
    assert_eq!(
        visible(&conversation, test_peers::bob()),
        [6, 7],
        "both, in the author's order"
    );
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 0);
}

/// A1 — why the defect fired on *every* restart: D12 persists the sender's
/// counter with its keypair while D7 leaves the receiver's mark in memory, so
/// the first sequence a fresh log hears is wherever that author's counter had
/// reached.
#[test]
fn an_author_resuming_at_a_high_sequence_against_a_fresh_log_reports_no_loss() {
    let mut conversation = Conversation::rehydrate(
        ConversationId::Broadcast,
        test_peers::alice(),
        Some(seq(41)),
    )
    .expect("the broadcast channel is never a conversation with oneself");

    accept(
        &mut conversation,
        test_peers::bob(),
        5_897,
        "after a restart",
    );
    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(closed, Vec::new(), "a restart is not loss");
    assert_eq!(visible(&conversation, test_peers::bob()), [5_897]);
    assert_eq!(
        conversation.high_water_mark(&test_peers::alice()),
        Some(seq(41)),
        "the local author's rehydrated mark is untouched by a remote origin"
    );
}

/// A2 — an arrival below an origin established by first sight is still loss,
/// reported and never mistaken for a duplicate (invariant 6, AC15).
#[test]
fn a_message_below_an_origin_established_by_first_sight_is_reported_not_dropped() {
    let mut conversation = broadcast();
    accept(
        &mut conversation,
        test_peers::bob(),
        6,
        "the first thing heard",
    );
    conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);
    let before = conversation.clone();

    let outcome = accept(&mut conversation, test_peers::bob(), 5, "below the origin");

    assert_eq!(
        rejection_of(&outcome),
        RejectionReason::ArrivedAfterGapClosed
    );
    assert!(
        !outcome.is_duplicate(),
        "this peer never held 5; calling it a duplicate would hide the loss"
    );
    assert!(outcome.closed_gap().is_none());
    assert_eq!(conversation, before, "the read view is unchanged");
    assert_eq!(visible(&conversation, test_peers::bob()), [6]);
}

/// A2 — the buffer-full trigger above an *established* origin still reports the
/// run it gave up on, with `BufferFull` as the cause.
#[test]
fn a_full_buffer_above_an_established_origin_still_reports_what_it_gave_up_on() {
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;
    accept(&mut conversation, test_peers::bob(), 1, "first");

    // 2 never arrives, so 3..=cap+2 fill the buffer exactly.
    for sequence in 3..=(cap + 2) {
        assert!(accept(&mut conversation, test_peers::bob(), sequence, "held").is_buffered());
    }

    let outcome = accept(
        &mut conversation,
        test_peers::bob(),
        cap + 3,
        "one more than fits",
    );

    assert_eq!(
        outcome.closed_gap(),
        Some(MessageGapClosed {
            conversation: ConversationId::Broadcast,
            author: test_peers::bob(),
            from: seq(2),
            to: seq(2),
            cause: GapCloseCause::BufferFull,
        })
    );
    assert!(outcome.is_applied());
    assert_eq!(
        visible(&conversation, test_peers::bob()),
        std::iter::once(1).chain(3..=(cap + 3)).collect::<Vec<_>>()
    );
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 0);
}

// ------------------------------------------------ rule R: abandoning a gap

/// Required case 1 — a late joiner must not starve (AC10), and must not be told
/// that the history it was never sent was lost (D10).
#[test]
fn a_late_joiner_starts_at_the_first_sequence_it_sees_once_the_window_elapses() {
    let mut conversation = broadcast();

    let outcome = accept(&mut conversation, test_peers::bob(), 47, "mid-stream");
    assert!(outcome.is_buffered());
    assert_eq!(
        conversation.high_water_mark(&test_peers::bob()),
        None,
        "an unproven initial gap commits the log to nothing"
    );
    assert!(visible(&conversation, test_peers::bob()).is_empty());

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(
        closed,
        Vec::new(),
        "the window ended by starting this author's stream at 47; 1..=46 were \
         never in flight to this peer, so none of them is loss"
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [47]);
    assert_eq!(
        conversation.high_water_mark(&test_peers::bob()),
        Some(seq(47))
    );

    let outcome = accept(&mut conversation, test_peers::bob(), 48, "the next one");

    assert!(outcome.is_applied());
    assert!(outcome.closed_gap().is_none());
    assert_eq!(visible(&conversation, test_peers::bob()), [47, 48]);
}

/// Required case 2 — genesis is provable, so it must not be made to wait
/// (AC1/AC2 first-contact latency).
#[test]
fn the_first_sequence_applies_at_once_with_no_settling_delay() {
    let mut conversation = broadcast();

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "hello");

    assert!(outcome.is_applied());
    assert_eq!(visible(&conversation, test_peers::bob()), [1]);
    assert!(
        conversation
            .close_aged_gaps(tolerance_elapsed(), TOLERANCE)
            .is_empty(),
        "there was never a gap to abandon"
    );
}

/// Required case 3 — reordering *inside* the window at first contact. The
/// rejected first-observed-baseline approach loses 3 and 4 here; rule R keeps
/// them, in the author's order.
#[test]
fn reordering_inside_the_window_at_first_contact_loses_nothing() {
    let mut conversation = broadcast();

    accept(&mut conversation, test_peers::bob(), 5, "fifth");
    accept(&mut conversation, test_peers::bob(), 3, "third");
    accept(&mut conversation, test_peers::bob(), 4, "fourth");
    assert!(visible(&conversation, test_peers::bob()).is_empty());

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(
        visible(&conversation, test_peers::bob()),
        [3, 4, 5],
        "every message received inside the window is displayed, in send order"
    );
    assert_eq!(
        closed,
        Vec::new(),
        "no gap is reported inside the run that arrived, and none below it \
         either: 3 is the lowest sequence this peer ever saw from bob, so the \
         stream starts there (D10)"
    );
    assert_eq!(
        conversation.high_water_mark(&test_peers::bob()),
        Some(seq(5))
    );
}

/// Required case 4 — reordering *beyond* the window. What arrives after its
/// gap closed is reported as loss, never as a duplicate.
#[test]
fn a_message_arriving_after_its_gap_closed_is_rejected_not_ignored() {
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 5, "fifth");
    conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);
    let before = conversation.clone();

    let outcome = accept(
        &mut conversation,
        test_peers::bob(),
        3,
        "third, far too late",
    );

    assert_eq!(
        rejection_of(&outcome),
        RejectionReason::ArrivedAfterGapClosed
    );
    assert!(
        !outcome.is_duplicate(),
        "it is not a duplicate: saying so would hide the loss (invariant 6)"
    );
    assert_eq!(conversation, before, "the read view is unchanged");
    assert_eq!(visible(&conversation, test_peers::bob()), [5]);
}

/// Required case 5 — a permanent mid-stream gap must not block the author's
/// whole stream.
#[test]
fn a_permanent_mid_stream_gap_is_abandoned_and_the_stream_resumes() {
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 1, "first");
    accept(&mut conversation, test_peers::bob(), 2, "second");
    accept(&mut conversation, test_peers::bob(), 4, "fourth");
    accept(&mut conversation, test_peers::bob(), 5, "fifth");
    assert_eq!(visible(&conversation, test_peers::bob()), [1, 2]);

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    assert_eq!(
        closed,
        vec![MessageGapClosed {
            conversation: ConversationId::Broadcast,
            author: test_peers::bob(),
            from: seq(3),
            to: seq(3),
            cause: GapCloseCause::ToleranceElapsed,
        }]
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [1, 2, 4, 5]);
}

/// Required case 6 — a full buffer closes the gap rather than refusing the
/// arrival, which is what makes the held content visible instead of lost.
#[test]
fn a_full_buffer_closes_the_gap_instead_of_refusing_the_arrival() {
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;

    // Sequence 1 never arrives, so 2..=cap+1 fill the buffer exactly.
    for sequence in 2..=(cap + 1) {
        assert!(accept(&mut conversation, test_peers::bob(), sequence, "held").is_buffered());
    }
    assert_eq!(
        conversation.buffered_count(&test_peers::bob()),
        cap as usize
    );

    let outcome = accept(
        &mut conversation,
        test_peers::bob(),
        cap + 2,
        "one more than fits",
    );

    assert_eq!(
        outcome.closed_gap(),
        None,
        "the buffer filling is what ended the wait, but this was first sight \
         of bob: the stream starts at 2 and nothing below it was lost (D10)"
    );
    assert!(outcome.is_applied(), "the arrival is taken, never refused");
    assert_eq!(
        became_visible(&outcome),
        (2..=(cap + 2)).collect::<Vec<_>>(),
        "everything held becomes visible, in send order, with the arrival last"
    );
    assert_eq!(
        visible(&conversation, test_peers::bob()),
        (2..=(cap + 2)).collect::<Vec<_>>()
    );
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 0);
}

#[test]
fn a_close_that_makes_room_still_reports_what_it_released_when_the_arrival_waits() {
    // The arrival is far ahead of the run the close releases, so it goes back
    // into the buffer — and everything the close made visible is still reported,
    // or the application would never learn to display it.
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;
    accept(&mut conversation, test_peers::bob(), 1, "first");
    for sequence in 3..=(cap + 2) {
        accept(&mut conversation, test_peers::bob(), sequence, "held");
    }

    let outcome = accept(&mut conversation, test_peers::bob(), 1_000, "far ahead");

    assert!(outcome.is_buffered());
    assert_eq!(
        outcome.closed_gap().map(|event| (event.from, event.to)),
        Some((seq(2), seq(2))),
        "2 was in flight between two observed sequences, so it is loss (A2)"
    );
    assert_eq!(
        became_visible(&outcome),
        (3..=(cap + 2)).collect::<Vec<_>>()
    );
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 1);
}

#[test]
fn a_first_sight_close_that_makes_room_still_reports_what_it_released() {
    // The same shape at first contact: nothing is abandoned, but the run the
    // close released is still reported, or it would never be displayed.
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;
    for sequence in 2..=(cap + 1) {
        accept(&mut conversation, test_peers::bob(), sequence, "held");
    }

    let outcome = accept(&mut conversation, test_peers::bob(), 1_000, "far ahead");

    assert!(outcome.is_buffered());
    assert_eq!(outcome.closed_gap(), None);
    assert_eq!(
        became_visible(&outcome),
        (2..=(cap + 1)).collect::<Vec<_>>()
    );
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 1);
}

#[test]
fn a_close_that_makes_room_can_leave_the_arrival_itself_out_of_reach() {
    // Filling the buffer above a gap and only then sending what belongs inside
    // it is how a peer would try to have its own late message applied out of
    // order. The close releases the held run; the arrival is refused, named.
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;
    for sequence in 10..(10 + cap) {
        accept(&mut conversation, test_peers::bob(), sequence, "held");
    }

    let outcome = accept(&mut conversation, test_peers::bob(), 5, "inside the gap");

    assert_eq!(
        outcome.closed_gap(),
        None,
        "first sight of bob: the stream starts at 10, and 1..=9 were never in \
         flight here"
    );
    assert_eq!(
        rejection_of(&outcome),
        RejectionReason::ArrivedAfterGapClosed,
        "below the origin is still refused and still named — never silent"
    );
    assert!(
        !outcome.is_duplicate(),
        "this peer never held 5; calling it a duplicate would hide it"
    );
    assert_eq!(
        became_visible(&outcome),
        (10..(10 + cap)).collect::<Vec<_>>()
    );
}

/// Required case 7 — no false diagnostics: an ordinary reorder that resolves
/// inside the window reports nothing.
#[test]
fn a_gap_that_closes_before_the_window_elapses_reports_nothing() {
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 1, "first");
    accept(&mut conversation, test_peers::bob(), 3, "third");
    accept(&mut conversation, test_peers::bob(), 2, "second");

    assert!(
        conversation
            .close_aged_gaps(tolerance_elapsed(), TOLERANCE)
            .is_empty()
    );
    assert_eq!(visible(&conversation, test_peers::bob()), [1, 2, 3]);
}

/// Required case 8 — the sweep is idempotent.
#[test]
fn a_second_sweep_with_nothing_new_changes_nothing() {
    let mut conversation = broadcast();
    // 1 first, so the sweep below abandons a run that genuinely was in flight
    // rather than merely establishing where the stream starts.
    accept(&mut conversation, test_peers::bob(), 1, "first");
    accept(&mut conversation, test_peers::bob(), 9, "mid-stream");
    assert_eq!(
        conversation
            .close_aged_gaps(tolerance_elapsed(), TOLERANCE)
            .len(),
        1
    );
    let after_first = conversation.clone();

    let second = conversation.close_aged_gaps(at(tolerance_elapsed().as_millis() * 4), TOLERANCE);

    assert!(second.is_empty());
    assert_eq!(conversation, after_first);
}

/// Required case 9 — determinism across authors (AC13).
#[test]
fn aged_gaps_close_in_peer_id_order() {
    let mut conversation = broadcast();
    let arrival_order = [test_peers::carol(), test_peers::bob(), test_peers::dave()];
    for author in arrival_order {
        // Each author's stream starts at 1 here, so the sweep abandons a run
        // that genuinely was in flight from each of them.
        accept(&mut conversation, author, 1, "first");
        accept(&mut conversation, author, 9, "mid-stream");
    }

    let closed = conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);

    let authors: Vec<PeerId> = closed.iter().map(|event| event.author).collect();
    let mut expected = arrival_order.to_vec();
    expected.sort();
    assert_eq!(authors, expected);
    assert_ne!(
        authors,
        arrival_order.to_vec(),
        "the fixture would prove nothing if arrival order already were PeerId order"
    );
}

/// Required case 10 — after a skip, a real duplicate and a real loss are told
/// apart. This is the defect `is_applied` by high-water comparison hid.
#[test]
fn after_a_skip_a_duplicate_and_a_lost_message_are_told_apart() {
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 47, "mid-stream");
    conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);
    let before = conversation.clone();

    let replayed = accept(&mut conversation, test_peers::bob(), 47, "mid-stream");
    assert_eq!(
        *replayed.placement(),
        MessagePlacement::DuplicateIgnored(MessageDuplicateIgnored {
            id: MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(47)),
        })
    );

    let inside_the_gap = accept(&mut conversation, test_peers::bob(), 46, "never arrived");
    assert_eq!(
        rejection_of(&inside_the_gap),
        RejectionReason::ArrivedAfterGapClosed
    );

    assert_eq!(conversation, before, "neither one changed any state");
}

/// Required case 11 — a skipped sequence is never reachable as a message.
#[test]
fn a_skipped_sequence_is_never_reachable_by_identifier() {
    let mut conversation = broadcast();
    let skipped = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(46));
    accept(&mut conversation, test_peers::bob(), 47, "mid-stream");
    assert!(conversation.message(&skipped).is_none());

    conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE);
    assert!(conversation.message(&skipped).is_none());

    accept(&mut conversation, test_peers::bob(), 46, "far too late");
    assert!(conversation.message(&skipped).is_none());
}

/// Required case 12 — the rule is identical for `Direct` (invariant 5).
#[test]
fn a_direct_conversation_follows_the_same_rule_as_the_broadcast_channel() {
    for id in [
        ConversationId::Broadcast,
        ConversationId::Direct(test_peers::bob()),
    ] {
        let build = || match id {
            ConversationId::Broadcast => broadcast(),
            ConversationId::Direct(_) => direct(),
        };

        // Case 1: a late joiner. First sight starts the stream at 47 and
        // reports no loss (D10).
        let mut conversation = build();
        accept(&mut conversation, test_peers::bob(), 47, "mid-stream");
        assert_eq!(
            conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE),
            Vec::new(),
            "{id:?}"
        );
        assert_eq!(visible(&conversation, test_peers::bob()), [47], "{id:?}");

        // Case 4: an arrival after its gap closed.
        let outcome = accept(&mut conversation, test_peers::bob(), 3, "far too late");
        assert_eq!(
            rejection_of(&outcome),
            RejectionReason::ArrivedAfterGapClosed,
            "{id:?}"
        );
        assert_eq!(visible(&conversation, test_peers::bob()), [47], "{id:?}");

        // Case 5: a permanent mid-stream gap.
        let mut conversation = build();
        accept(&mut conversation, test_peers::bob(), 1, "first");
        accept(&mut conversation, test_peers::bob(), 2, "second");
        accept(&mut conversation, test_peers::bob(), 4, "fourth");
        accept(&mut conversation, test_peers::bob(), 5, "fifth");
        assert_eq!(
            conversation.close_aged_gaps(tolerance_elapsed(), TOLERANCE),
            vec![MessageGapClosed {
                conversation: id,
                author: test_peers::bob(),
                from: seq(3),
                to: seq(3),
                cause: GapCloseCause::ToleranceElapsed,
            }],
            "{id:?}"
        );
        assert_eq!(
            visible(&conversation, test_peers::bob()),
            [1, 2, 4, 5],
            "{id:?}"
        );
    }
}

/// Every permutation of `values[..k]`, visited in place (Heap's algorithm).
/// Exhaustive and deterministic — no randomness reaches a test (AC13, S5).
fn for_each_permutation(values: &mut Vec<u64>, k: usize, visit: &mut impl FnMut(&[u64])) {
    if k <= 1 {
        visit(values);
        return;
    }

    for index in 0..k {
        for_each_permutation(values, k - 1, visit);
        if k.is_multiple_of(2) {
            values.swap(index, k - 1);
        } else {
            values.swap(0, k - 1);
        }
    }
}

/// Required case 13 — property: however a complete run is scrambled in flight,
/// it displays in the author's send order and nothing is ever abandoned (AC8).
#[test]
fn any_arrival_order_of_a_complete_run_displays_in_send_order() {
    let mut order: Vec<u64> = (1..=8).collect();
    let length = order.len();
    let expected: Vec<u64> = order.clone();
    let mut checked = 0usize;

    for_each_permutation(&mut order, length, &mut |arrival| {
        let mut conversation = broadcast();
        for sequence in arrival {
            let outcome = accept(&mut conversation, test_peers::bob(), *sequence, "text");
            assert!(
                outcome.closed_gap().is_none(),
                "nothing is abandoned while the run is still arriving: {arrival:?}"
            );
        }

        assert_eq!(
            visible(&conversation, test_peers::bob()),
            expected,
            "{arrival:?}"
        );
        assert!(
            conversation
                .close_aged_gaps(tolerance_elapsed(), TOLERANCE)
                .is_empty(),
            "a complete run leaves no gap to abandon: {arrival:?}"
        );
        checked += 1;
    });

    assert_eq!(checked, 40_320, "every permutation of 1..=8 was checked");
}

// --------------------------------------------- the local instant is local

#[test]
fn ageing_follows_the_local_arrival_instant_not_the_authors_claim() {
    // The author's claimed send time is far in the future here; if it drove
    // ageing, the sweep below would never fire.
    let mut conversation = broadcast();
    accept_at(&mut conversation, test_peers::bob(), 1, "first", at(1));
    accept_at(&mut conversation, test_peers::bob(), 9, "mid-stream", at(1));

    let too_early = conversation.close_aged_gaps(at(TOLERANCE.as_millis()), TOLERANCE);
    assert!(too_early.is_empty(), "the window has not elapsed yet");

    let closed = conversation.close_aged_gaps(at(1 + TOLERANCE.as_millis()), TOLERANCE);
    assert_eq!(closed.len(), 1);
    assert_eq!((closed[0].from, closed[0].to), (seq(2), seq(8)));
}

#[test]
fn the_oldest_held_message_decides_when_a_gap_is_abandoned() {
    let mut conversation = broadcast();
    accept_at(&mut conversation, test_peers::bob(), 1, "first", at(100));
    accept_at(
        &mut conversation,
        test_peers::bob(),
        5,
        "held longest",
        at(100),
    );
    accept_at(
        &mut conversation,
        test_peers::bob(),
        6,
        "held briefly",
        at(900),
    );

    let closed = conversation.close_aged_gaps(at(100 + TOLERANCE.as_millis()), TOLERANCE);

    assert_eq!(closed.len(), 1, "5 aged, and 6 did not extend its wait");
    assert_eq!((closed[0].from, closed[0].to), (seq(2), seq(4)));
    assert_eq!(visible(&conversation, test_peers::bob()), [1, 5, 6]);
}

// --------------------------------------------------------------- duplicates

#[test]
fn an_already_applied_message_is_ignored() {
    // Invariant 6 / AC7: redelivery over any path changes nothing.
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 1, "first");

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "first");

    assert_eq!(
        *outcome.placement(),
        MessagePlacement::DuplicateIgnored(MessageDuplicateIgnored {
            id: MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(1)),
        })
    );
}

#[test]
fn a_message_below_the_high_water_mark_is_ignored_when_it_really_was_applied() {
    let mut conversation = broadcast();
    for sequence in 1..=3 {
        accept(&mut conversation, test_peers::bob(), sequence, "text");
    }

    assert!(accept(&mut conversation, test_peers::bob(), 2, "text").is_duplicate());
}

#[test]
fn a_redelivered_buffered_message_is_ignored_too() {
    // It is not lost and not applied — it is already held, so a second copy is
    // still a no-op.
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 4, "fourth");

    assert!(accept(&mut conversation, test_peers::bob(), 4, "fourth").is_duplicate());
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 1);
}

#[test]
fn a_duplicate_changes_no_state_at_all() {
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 1, "the original");
    let before = conversation.clone();

    // Even with different content under the same identifier: the first copy
    // applied is the one that counts, so a forger cannot rewrite history by
    // replaying an identifier.
    accept(&mut conversation, test_peers::bob(), 1, "a replacement");

    assert_eq!(conversation, before);
    assert_eq!(
        conversation.messages_by(&test_peers::bob())[0]
            .body()
            .as_str(),
        "the original"
    );
}

// ------------------------------------------------------- the bounded buffer

#[test]
fn a_full_buffer_still_accepts_the_message_that_closes_the_gap() {
    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES;
    for sequence in 2..=(cap as u64 + 1) {
        accept(&mut conversation, test_peers::bob(), sequence, "held");
    }

    let outcome = accept(&mut conversation, test_peers::bob(), 1, "first");

    assert!(
        outcome.closed_gap().is_none(),
        "nothing had to be abandoned"
    );
    assert_eq!(outcome.applied().len(), cap + 1);
    assert_eq!(conversation.buffered_count(&test_peers::bob()), 0);
    assert_eq!(conversation.applied_len(), cap + 1);
}

// ---------------------------------------------------- who may be in a message

#[test]
fn an_inbound_message_claiming_the_local_peer_as_author_is_rejected() {
    let mut conversation = broadcast();

    assert_eq!(
        conversation
            .accept_remote(
                test_peers::alice(),
                SequenceNumber::FIRST,
                body("hi"),
                SENT_AT,
                ARRIVED_AT,
            )
            .unwrap_err(),
        ConversationError::SelfAuthoredInbound
    );
}

#[test]
fn a_direct_conversation_admits_only_its_counterpart() {
    let mut conversation = direct();

    assert_eq!(
        conversation
            .accept_remote(
                test_peers::carol(),
                SequenceNumber::FIRST,
                body("hi"),
                SENT_AT,
                ARRIVED_AT,
            )
            .unwrap_err(),
        ConversationError::AuthorNotInConversation
    );
    assert!(conversation.is_empty());
}

#[test]
fn the_broadcast_channel_admits_every_author() {
    let mut conversation = broadcast();

    for author in [test_peers::bob(), test_peers::carol(), test_peers::dave()] {
        accept(&mut conversation, author, 1, "hello all");
    }

    assert_eq!(conversation.applied_len(), 3);
}

// ------------------------------------------------------ delivery transitions

#[test]
fn marking_a_pending_direct_message_delivered_reports_the_change() {
    let mut conversation = direct();
    let sent = conversation
        .append_local(body("hi"), SENT_AT)
        .expect("room");

    let change = conversation
        .mark_delivered(&sent.id)
        .expect("a pending message may be delivered");

    assert_eq!(change.id, sent.id);
    assert_eq!(change.from, DeliveryState::Pending);
    assert_eq!(change.to, DeliveryState::Delivered);
    assert_eq!(
        conversation
            .message(&sent.id)
            .expect("applied")
            .delivery_state(),
        DeliveryState::Delivered
    );
}

#[test]
fn marking_a_pending_direct_message_failed_names_the_reason() {
    // D10: the retry cycle ends in a stated failure the user can act on.
    let mut conversation = direct();
    let sent = conversation
        .append_local(body("hi"), SENT_AT)
        .expect("room");

    let change = conversation
        .mark_failed(&sent.id, DeliveryFailure::NoRelayAvailable)
        .expect("a pending message may fail");

    assert_eq!(
        change.to,
        DeliveryState::Failed(DeliveryFailure::NoRelayAvailable)
    );
}

#[test]
fn a_message_that_already_ended_cannot_be_marked_again() {
    let mut conversation = direct();
    let sent = conversation
        .append_local(body("hi"), SENT_AT)
        .expect("room");
    conversation.mark_delivered(&sent.id).expect("first mark");

    assert_eq!(
        conversation.mark_delivered(&sent.id).unwrap_err(),
        ConversationError::InvalidDeliveryTransition {
            from: DeliveryState::Delivered,
            to: DeliveryState::Delivered,
        }
    );
    assert_eq!(
        conversation
            .mark_failed(&sent.id, DeliveryFailure::SessionClosed)
            .unwrap_err(),
        ConversationError::InvalidDeliveryTransition {
            from: DeliveryState::Delivered,
            to: DeliveryState::Failed(DeliveryFailure::SessionClosed),
        }
    );
}

#[test]
fn a_broadcast_message_has_no_delivery_transition_to_make() {
    let mut conversation = broadcast();
    let sent = conversation
        .append_local(body("hi"), SENT_AT)
        .expect("room");

    assert_eq!(
        conversation.mark_delivered(&sent.id).unwrap_err(),
        ConversationError::InvalidDeliveryTransition {
            from: DeliveryState::Published,
            to: DeliveryState::Delivered,
        }
    );
}

#[test]
fn marking_a_message_this_conversation_does_not_hold_is_rejected() {
    let mut conversation = direct();
    let unknown = MessageId::new(
        test_peers::bob(),
        ConversationId::Direct(test_peers::bob()),
        seq(7),
    );

    assert_eq!(
        conversation.mark_delivered(&unknown).unwrap_err(),
        ConversationError::UnknownMessage
    );
}

#[test]
fn an_identifier_from_another_conversation_is_rejected() {
    let mut conversation = direct();
    let elsewhere = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(1));

    assert_eq!(
        conversation.mark_delivered(&elsewhere).unwrap_err(),
        ConversationError::WrongConversation
    );
}

#[test]
fn a_buffered_message_is_not_reachable_by_identifier() {
    // Buffered content is not in the read view, so nothing may look it up or
    // mark it (invariant 5).
    let mut conversation = broadcast();
    accept(&mut conversation, test_peers::bob(), 2, "held");
    let buffered = MessageId::new(test_peers::bob(), ConversationId::Broadcast, seq(2));

    assert!(conversation.message(&buffered).is_none());
}

// ------------------------------------------------------- failing what is due

#[test]
fn failing_the_pending_messages_reports_every_change_once() {
    // D10: a `PeerDisconnected` fails that peer's pending directs, and the
    // aggregate decides which those are.
    let mut conversation = direct();
    let first = conversation
        .append_local(body("one"), SENT_AT)
        .expect("room");
    let second = conversation
        .append_local(body("two"), SENT_AT)
        .expect("room");
    conversation
        .mark_delivered(&first.id)
        .expect("the first one landed");

    let changes = conversation.fail_pending(DeliveryFailure::SessionClosed);

    assert_eq!(changes.len(), 1, "only what was still pending");
    assert_eq!(changes[0].id, second.id);
    assert_eq!(changes[0].from, DeliveryState::Pending);
    assert_eq!(
        changes[0].to,
        DeliveryState::Failed(DeliveryFailure::SessionClosed)
    );
    assert_eq!(
        conversation
            .message(&first.id)
            .expect("applied")
            .delivery_state(),
        DeliveryState::Delivered,
        "a settled message is never re-opened"
    );
    assert!(
        conversation
            .fail_pending(DeliveryFailure::SessionClosed)
            .is_empty(),
        "nothing is left pending, so a second call reports nothing"
    );
}

#[test]
fn failing_the_pending_messages_leaves_broadcast_content_alone() {
    // Published is not pending: gossip has no acknowledgement to lose (D3).
    let mut conversation = broadcast();
    conversation
        .append_local(body("to everyone"), SENT_AT)
        .expect("room");
    accept(&mut conversation, test_peers::bob(), 1, "and back");
    let before = conversation.clone();

    assert!(
        conversation
            .fail_pending(DeliveryFailure::PeerUnreachable)
            .is_empty()
    );
    assert_eq!(conversation, before);
}

// -------------------------------------------------------- outcomes as events

#[test]
fn an_outcome_carries_exactly_the_events_the_application_must_publish() {
    let mut conversation = broadcast();

    let buffered = accept(&mut conversation, test_peers::bob(), 2, "second");
    assert!(
        buffered.into_events().is_empty(),
        "buffering is not something that happened to the conversation yet"
    );

    let applied = accept(&mut conversation, test_peers::bob(), 1, "first");
    assert_eq!(applied.into_events().len(), 2, "one per applied message");

    let duplicate = accept(&mut conversation, test_peers::bob(), 1, "first");
    assert_eq!(duplicate.into_events().len(), 1);
}

#[test]
fn an_abandoned_gap_is_published_before_the_messages_it_released() {
    use crate::domain::events::MessagingEvent;

    let mut conversation = broadcast();
    let cap = AuthorLog::MAX_BUFFERED_MESSAGES as u64;
    // 1 first, so 2 is a genuine loss between two observed sequences and there
    // is an abandonment to order (A2).
    accept(&mut conversation, test_peers::bob(), 1, "first");
    for sequence in 3..=(cap + 2) {
        accept(&mut conversation, test_peers::bob(), sequence, "held");
    }

    let events = accept(&mut conversation, test_peers::bob(), cap + 3, "one more").into_events();

    assert!(
        matches!(events.first(), Some(MessagingEvent::MessageGapClosed(_))),
        "the abandonment explains the jump in the messages that follow it"
    );
    assert_eq!(events.len(), 1 + cap as usize + 1);
}
