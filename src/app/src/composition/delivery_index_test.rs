use messaging::domain::{ConversationId, MessageId, SequenceNumber};
use shared_types::EnvelopeSignature;

use crate::composition::DeliveryIndex;
use crate::test_peers::{alice, bob};

fn signature(seed: u8) -> EnvelopeSignature {
    EnvelopeSignature::new([seed; EnvelopeSignature::LENGTH])
}

fn message(sequence: u64) -> MessageId {
    MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::new(sequence).expect("a non-zero sequence"),
    )
}

#[test]
fn a_recorded_signature_names_its_message() {
    let index = DeliveryIndex::new();
    index.record(signature(1), message(1));

    assert_eq!(index.take(&signature(1)), Some(message(1)));
}

#[test]
fn an_unknown_signature_names_nothing() {
    let index = DeliveryIndex::new();

    assert_eq!(index.take(&signature(9)), None);
}

#[test]
fn a_signature_is_answered_at_most_once() {
    // Delivered or failed, never both: the second report is a late duplicate
    // and must not re-mark a message the user was already told about.
    let index = DeliveryIndex::new();
    index.record(signature(1), message(1));

    assert_eq!(index.take(&signature(1)), Some(message(1)));
    assert_eq!(index.take(&signature(1)), None);
}

#[test]
fn taking_one_leaves_the_others() {
    let index = DeliveryIndex::new();
    index.record(signature(1), message(1));
    index.record(signature(2), message(2));

    assert_eq!(index.take(&signature(1)), Some(message(1)));
    assert_eq!(index.take(&signature(2)), Some(message(2)));
}

#[test]
fn outstanding_counts_what_nothing_has_answered_for() {
    let index = DeliveryIndex::new();
    index.record(signature(1), message(1));
    index.record(signature(2), message(2));
    index.take(&signature(1));

    assert_eq!(index.outstanding(), 1);
}

#[test]
fn the_oldest_unanswered_entry_is_evicted_past_the_cap() {
    // A peer that never acknowledges must not be able to make this process
    // hold one entry per message forever (S6).
    let index = DeliveryIndex::with_capacity(2);
    index.record(signature(1), message(1));
    index.record(signature(2), message(2));
    index.record(signature(3), message(3));

    assert_eq!(index.take(&signature(1)), None);
    assert_eq!(index.take(&signature(2)), Some(message(2)));
    assert_eq!(index.take(&signature(3)), Some(message(3)));
}

#[test]
fn re_recording_a_signature_does_not_grow_the_queue() {
    let index = DeliveryIndex::with_capacity(4);

    index.record(signature(1), message(1));
    index.record(signature(1), message(2));

    assert_eq!(index.outstanding(), 1);
    assert_eq!(index.take(&signature(1)), Some(message(2)));
}
