use crate::domain::DurationMillis;

#[test]
fn from_secs_converts_to_milliseconds() {
    assert_eq!(DurationMillis::from_secs(30).as_millis(), 30_000);
}

#[test]
fn from_secs_saturates_instead_of_overflowing() {
    assert_eq!(
        DurationMillis::from_secs(u64::MAX),
        DurationMillis::from_millis(u64::MAX)
    );
}

#[test]
fn zero_is_the_empty_span() {
    assert_eq!(DurationMillis::ZERO, DurationMillis::from_millis(0));
    assert_eq!(DurationMillis::ZERO.as_millis(), 0);
}

#[test]
fn orders_by_length() {
    assert!(DurationMillis::from_millis(1) < DurationMillis::from_millis(2));
    assert!(DurationMillis::from_secs(1) > DurationMillis::from_millis(999));
}

#[test]
fn displays_a_millisecond_reading() {
    assert_eq!(DurationMillis::from_secs(2).to_string(), "2000ms");
}
