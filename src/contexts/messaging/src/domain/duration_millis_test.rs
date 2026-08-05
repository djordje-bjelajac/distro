use crate::domain::DurationMillis;

#[test]
fn a_span_round_trips_through_its_millisecond_value() {
    assert_eq!(DurationMillis::from_millis(2_000).as_millis(), 2_000);
    assert_eq!(DurationMillis::ZERO.as_millis(), 0);
}

#[test]
fn from_secs_converts_to_milliseconds() {
    assert_eq!(DurationMillis::from_secs(2).as_millis(), 2_000);
}

#[test]
fn from_secs_saturates_instead_of_overflowing() {
    assert_eq!(
        DurationMillis::from_secs(u64::MAX),
        DurationMillis::from_millis(u64::MAX)
    );
}

#[test]
fn spans_order_by_length() {
    assert!(DurationMillis::from_millis(1) < DurationMillis::from_millis(2));
    assert!(DurationMillis::from_secs(1) > DurationMillis::from_millis(999));
}

#[test]
fn a_span_renders_with_its_unit() {
    assert_eq!(DurationMillis::from_secs(2).to_string(), "2000ms");
}
