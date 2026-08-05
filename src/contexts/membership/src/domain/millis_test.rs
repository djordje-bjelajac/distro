use crate::domain::{DurationMillis, Millis};

#[test]
fn orders_by_position_on_the_timeline() {
    assert!(Millis::ZERO < Millis::from_millis(1));
    assert!(Millis::from_millis(10) < Millis::MAX);
}

#[test]
fn elapsed_since_measures_the_span_from_the_earlier_instant() {
    let earlier = Millis::from_millis(1_000);
    let later = Millis::from_millis(4_500);

    assert_eq!(
        later.saturating_elapsed_since(earlier),
        DurationMillis::from_millis(3_500)
    );
}

#[test]
fn elapsed_since_the_same_instant_is_zero() {
    let at = Millis::from_millis(7);

    assert_eq!(at.saturating_elapsed_since(at), DurationMillis::ZERO);
}

#[test]
fn elapsed_since_a_later_instant_is_zero_rather_than_underflow() {
    // The clock behind `ClockPort` is monotonic (D11), so a "negative" age can
    // only be a caller mistake; the domain reports no age at all rather than
    // wrapping into a span of ~584 million years.
    let earlier = Millis::from_millis(10);
    let later = Millis::from_millis(4_000);

    assert_eq!(
        earlier.saturating_elapsed_since(later),
        DurationMillis::ZERO
    );
}

#[test]
fn saturating_add_moves_forward_by_a_span() {
    assert_eq!(
        Millis::from_millis(500).saturating_add(DurationMillis::from_secs(2)),
        Millis::from_millis(2_500)
    );
}

#[test]
fn saturating_add_clamps_at_the_end_of_the_timeline() {
    assert_eq!(
        Millis::MAX.saturating_add(DurationMillis::from_millis(1)),
        Millis::MAX
    );
}

#[test]
fn displays_a_millisecond_reading() {
    assert_eq!(Millis::from_millis(1_500).to_string(), "1500ms");
}
