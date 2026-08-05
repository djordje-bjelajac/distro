use crate::domain::{SequenceNumber, SequenceNumberError};

#[test]
fn the_first_sequence_number_is_one_so_zero_can_mean_no_message_yet() {
    assert_eq!(SequenceNumber::FIRST.as_u64(), 1);
}

#[test]
fn zero_is_not_a_sequence_number() {
    assert_eq!(SequenceNumber::new(0), Err(SequenceNumberError::Zero));
}

#[test]
fn any_positive_value_is_a_sequence_number() {
    for value in [1u64, 2, 99, u64::MAX] {
        assert_eq!(
            SequenceNumber::new(value).map(SequenceNumber::as_u64),
            Ok(value)
        );
    }
}

#[test]
fn the_successor_is_strictly_greater_by_one() {
    let first = SequenceNumber::FIRST;
    let second = first.successor().expect("room to grow");

    assert_eq!(second.as_u64(), 2);
    assert!(second > first);
}

#[test]
fn the_predecessor_is_strictly_smaller_by_one() {
    let fifth = SequenceNumber::new(5).expect("positive");

    assert_eq!(fifth.predecessor(), SequenceNumber::new(4).ok());
    assert_eq!(
        SequenceNumber::MAX.predecessor(),
        SequenceNumber::new(u64::MAX - 1).ok()
    );
}

#[test]
fn the_first_sequence_number_has_no_predecessor() {
    // Abandoning a gap moves a log's mark to the number below the lowest
    // message it holds — and a gap below the first number cannot exist, because
    // nothing precedes genesis.
    assert_eq!(SequenceNumber::FIRST.predecessor(), None);
}

#[test]
fn the_last_sequence_number_has_no_successor() {
    assert_eq!(
        SequenceNumber::MAX.successor(),
        Err(SequenceNumberError::Exhausted)
    );
}

#[test]
fn the_number_following_nothing_is_the_first_one() {
    assert_eq!(SequenceNumber::following(None), Ok(SequenceNumber::FIRST));
}

#[test]
fn the_number_following_a_high_water_mark_is_its_successor() {
    let mark = SequenceNumber::new(41).expect("positive");

    assert_eq!(SequenceNumber::following(Some(mark)), mark.successor());
}

#[test]
fn following_the_last_sequence_number_is_exhaustion() {
    assert_eq!(
        SequenceNumber::following(Some(SequenceNumber::MAX)),
        Err(SequenceNumberError::Exhausted)
    );
}

#[test]
fn a_run_of_successors_is_strictly_monotonic() {
    let mut current = SequenceNumber::FIRST;
    let mut previous = None;

    for _ in 0..10 {
        if let Some(previous) = previous {
            assert!(current > previous);
        }
        previous = Some(current);
        current = current.successor().expect("room to grow");
    }
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        SequenceNumberError::Zero.to_string(),
        "0 is not a sequence number; the first message carries 1"
    );
    assert_eq!(
        SequenceNumberError::Exhausted.to_string(),
        "no sequence number remains after the last one"
    );
}
