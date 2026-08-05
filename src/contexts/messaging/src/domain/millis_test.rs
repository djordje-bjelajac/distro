use crate::domain::{DurationMillis, Millis};

#[test]
fn a_reading_round_trips_through_its_millisecond_value() {
    assert_eq!(
        Millis::from_millis(1_700_000_000_000).as_millis(),
        1_700_000_000_000
    );
    assert_eq!(Millis::ZERO.as_millis(), 0);
}

#[test]
fn readings_order_by_their_millisecond_value() {
    assert!(Millis::from_millis(1) < Millis::from_millis(2));
    assert_eq!(Millis::from_millis(7), Millis::from_millis(7));
}

#[test]
fn a_reading_renders_with_its_unit() {
    assert_eq!(Millis::from_millis(42).to_string(), "42ms");
}

#[test]
fn an_age_is_the_distance_back_to_an_earlier_reading() {
    assert_eq!(
        Millis::from_millis(2_500).saturating_elapsed_since(Millis::from_millis(500)),
        DurationMillis::from_millis(2_000)
    );
    assert_eq!(
        Millis::from_millis(7).saturating_elapsed_since(Millis::from_millis(7)),
        DurationMillis::ZERO
    );
}

#[test]
fn an_age_saturates_at_zero_rather_than_wrapping() {
    // A reading that precedes the arrival would mean the monotonic clock ran
    // backwards. Wrapping would produce an enormous age and abandon every open
    // gap at once, which is the opposite of what a clock glitch should cause.
    assert_eq!(
        Millis::from_millis(1).saturating_elapsed_since(Millis::from_millis(1_000)),
        DurationMillis::ZERO
    );
}
