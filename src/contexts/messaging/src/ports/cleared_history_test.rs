use crate::ports::ClearedHistory;

/// The two counts answer different questions, and a clear that dropped
/// conversations nobody had spoken in still did something.
#[test]
fn conversations_and_messages_are_counted_separately() {
    let cleared = ClearedHistory {
        conversations_dropped: 6,
        messages_dropped: 0,
    };

    assert!(!cleared.is_empty());
    assert_eq!(cleared.conversations_dropped, 6);
    assert_eq!(cleared.messages_dropped, 0);
}

#[test]
fn a_clear_that_found_nothing_says_so() {
    assert!(ClearedHistory::default().is_empty());
}

#[test]
fn a_clear_that_dropped_only_messages_is_not_empty() {
    let cleared = ClearedHistory {
        conversations_dropped: 0,
        messages_dropped: 3,
    };

    assert!(!cleared.is_empty());
}
