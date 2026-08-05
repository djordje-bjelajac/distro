use messaging::domain::events::{GapCloseCause, MessageGapClosed};
use messaging::domain::{
    ConversationId, DeliveryFailure, DeliveryState, Message, MessageBody, MessageId, Millis,
    SequenceNumber,
};
use shared_types::PeerId;

use crate::test_peers::{alice, bob};
use crate::tui::{ConversationView, Entry, PeerLabels, delivery_text};

fn sequence(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("a non-zero sequence")
}

fn received(author: PeerId, value: u64, body: &str) -> Message {
    Message::received(
        MessageId::new(author, ConversationId::Broadcast, sequence(value)),
        MessageBody::new(body).expect("an admissible body"),
        Millis::from_millis(value),
    )
}

fn sent(to: PeerId, author: PeerId, value: u64, body: &str) -> Message {
    Message::outbound(
        MessageId::new(author, ConversationId::Direct(to), sequence(value)),
        MessageBody::new(body).expect("an admissible body"),
        Millis::from_millis(value),
    )
}

fn gap(author: PeerId, from: u64, to: u64, cause: GapCloseCause) -> MessageGapClosed {
    MessageGapClosed {
        conversation: ConversationId::Broadcast,
        author,
        from: sequence(from),
        to: sequence(to),
        cause,
    }
}

fn labels() -> PeerLabels {
    PeerLabels::for_local(alice())
}

fn bodies(view: &ConversationView) -> Vec<String> {
    view.authors
        .iter()
        .flat_map(|run| {
            run.entries.iter().filter_map(|entry| match entry {
                Entry::Message { body, .. } => Some(body.clone()),
                Entry::AbandonedRun { .. } => None,
            })
        })
        .collect()
}

#[test]
fn an_empty_conversation_draws_nothing() {
    let view = ConversationView::build(&[], &[], labels());

    assert!(view.is_empty());
    assert!(view.authors.is_empty());
}

#[test]
fn one_authors_messages_keep_that_authors_send_order() {
    // AC8, and the only ordering claim the domain makes.
    let view = ConversationView::build(
        &[
            received(bob(), 1, "first"),
            received(bob(), 2, "second"),
            received(bob(), 3, "third"),
        ],
        &[],
        labels(),
    );

    assert_eq!(bodies(&view), vec!["first", "second", "third"]);
}

#[test]
fn two_authors_are_two_blocks_and_are_not_interleaved() {
    // The read model groups by author and provides no cross-author order.
    // Merging them into one column would be inventing a chronology out of two
    // unsynchronised clocks.
    let view = ConversationView::build(
        &[
            sent(bob(), alice(), 1, "mine"),
            sent(bob(), bob(), 1, "theirs"),
        ],
        &[],
        labels(),
    );

    assert_eq!(view.authors.len(), 2);
    assert_eq!(view.authors[0].author, alice());
    assert_eq!(view.authors[1].author, bob());
}

#[test]
fn the_local_peer_is_labelled_and_marked_as_itself() {
    let view = ConversationView::build(&[sent(bob(), alice(), 1, "mine")], &[], labels());

    assert_eq!(view.authors[0].label, "you");
    assert!(view.authors[0].is_local);
}

#[test]
fn a_remote_author_is_labelled_by_its_fingerprint() {
    // Never by a name it chose for itself: invariant 8, and a name is the one
    // field an impersonator would set.
    let view = ConversationView::build(&[received(bob(), 1, "hello")], &[], labels());

    assert_eq!(view.authors[0].label, PeerLabels::short(bob()));
    assert!(!view.authors[0].is_local);
}

#[test]
fn an_abandoned_run_is_placed_where_the_hole_is() {
    // A marker at the bottom would say something was lost without saying
    // where.
    let view = ConversationView::build(
        &[received(bob(), 1, "before"), received(bob(), 5, "after")],
        &[gap(bob(), 2, 4, GapCloseCause::ToleranceElapsed)],
        labels(),
    );

    let entries = &view.authors[0].entries;
    assert!(matches!(entries[0], Entry::Message { .. }));
    assert!(matches!(
        entries[1],
        Entry::AbandonedRun { messages: 3, .. }
    ));
    assert!(matches!(entries[2], Entry::Message { .. }));
}

#[test]
fn an_abandoned_run_at_the_end_of_what_is_known_is_shown_last() {
    let view = ConversationView::build(
        &[received(bob(), 1, "before")],
        &[gap(bob(), 2, 2, GapCloseCause::ToleranceElapsed)],
        labels(),
    );

    assert!(matches!(
        view.authors[0].entries[1],
        Entry::AbandonedRun { messages: 1, .. }
    ));
}

#[test]
fn several_abandoned_runs_stay_in_sequence_order() {
    let view = ConversationView::build(
        &[
            received(bob(), 1, "a"),
            received(bob(), 4, "b"),
            received(bob(), 8, "c"),
        ],
        &[
            gap(bob(), 5, 7, GapCloseCause::BufferFull),
            gap(bob(), 2, 3, GapCloseCause::ToleranceElapsed),
        ],
        labels(),
    );

    let entries = &view.authors[0].entries;
    let abandoned: Vec<u64> = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::AbandonedRun { from, .. } => Some(from.as_u64()),
            Entry::Message { .. } => None,
        })
        .collect();

    assert_eq!(abandoned, vec![2, 5]);
    assert!(matches!(entries[0], Entry::Message { .. }));
    assert!(matches!(entries[1], Entry::AbandonedRun { .. }));
    assert!(matches!(entries[2], Entry::Message { .. }));
    assert!(matches!(entries[3], Entry::AbandonedRun { .. }));
    assert!(matches!(entries[4], Entry::Message { .. }));
}

#[test]
fn an_author_whose_every_message_was_abandoned_still_appears() {
    // Otherwise the loss is invisible, which is the one thing AC15 forbids.
    let view = ConversationView::build(
        &[],
        &[gap(bob(), 1, 3, GapCloseCause::ToleranceElapsed)],
        labels(),
    );

    assert_eq!(view.authors.len(), 1);
    assert_eq!(view.authors[0].author, bob());
}

#[test]
fn the_abandoned_sentence_names_the_count_and_the_author() {
    // AC15 asks for exactly this to be visible in the conversation.
    let entry = Entry::AbandonedRun {
        from: sequence(2),
        to: sequence(4),
        messages: 3,
        cause: GapCloseCause::ToleranceElapsed,
    };

    let text = entry.abandoned_text("21fe 31df").expect("an abandoned run");

    assert!(text.contains("3 messages"), "{text}");
    assert!(text.contains("21fe 31df"), "{text}");
    assert!(text.contains("never received"), "{text}");
}

#[test]
fn one_abandoned_message_reads_in_the_singular() {
    let entry = Entry::AbandonedRun {
        from: sequence(2),
        to: sequence(2),
        messages: 1,
        cause: GapCloseCause::BufferFull,
    };

    let text = entry.abandoned_text("peer").expect("an abandoned run");

    assert!(text.contains("1 message from"), "{text}");
    assert!(!text.contains("1 messages"), "{text}");
}

#[test]
fn the_two_close_causes_read_differently() {
    // A slow path and a flooding peer are different faults, and the cause is
    // the only thing that tells them apart.
    let elapsed = Entry::AbandonedRun {
        from: sequence(1),
        to: sequence(1),
        messages: 1,
        cause: GapCloseCause::ToleranceElapsed,
    };
    let full = Entry::AbandonedRun {
        from: sequence(1),
        to: sequence(1),
        messages: 1,
        cause: GapCloseCause::BufferFull,
    };

    assert_ne!(elapsed.abandoned_text("peer"), full.abandoned_text("peer"));
}

#[test]
fn a_message_has_no_abandoned_sentence() {
    let entry = Entry::Message {
        sequence: sequence(1),
        body: "hello".to_owned(),
        delivery: DeliveryState::Published,
    };

    assert_eq!(entry.abandoned_text("peer"), None);
}

#[test]
fn every_delivery_state_shows_a_mark_and_a_failure_shows_its_reason() {
    // AC11: silent loss is not a state, so there is no blank mark.
    assert!(delivery_text(DeliveryState::Pending).contains("pending"));
    assert!(delivery_text(DeliveryState::Delivered).contains("delivered"));
    assert!(delivery_text(DeliveryState::Published).contains("published"));

    let failed = delivery_text(DeliveryState::Failed(DeliveryFailure::NoRelayAvailable));
    assert!(failed.starts_with('✗'), "{failed}");
    assert!(failed.contains("relay"), "{failed}");
}

#[test]
fn a_block_is_headed_by_its_author_and_holds_that_authors_entries() {
    let view = ConversationView::build(&[received(bob(), 1, "hello")], &[], labels());

    assert_eq!(view.authors.len(), 1);
    assert_eq!(view.authors[0].label, PeerLabels::short(bob()));
    assert!(matches!(view.authors[0].entries[0], Entry::Message { .. }));
}
