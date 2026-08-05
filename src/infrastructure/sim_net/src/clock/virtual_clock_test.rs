use std::sync::Arc;

use membership::ports::ClockPort as MembershipClock;
use messaging::ports::ClockPort as MessagingClock;

use crate::clock::VirtualClock;

#[test]
fn a_new_clock_stands_at_its_epoch() {
    let clock = VirtualClock::new();

    assert_eq!(clock.now_millis(), VirtualClock::EPOCH_MILLIS);
}

#[test]
fn the_clock_never_moves_on_its_own() {
    let clock = VirtualClock::starting_at(500);

    for _ in 0..1_000 {
        assert_eq!(clock.now_millis(), 500);
        assert_eq!(clock.membership_now().as_millis(), 500);
        assert_eq!(clock.messaging_now().as_millis(), 500);
    }
}

#[test]
fn advancing_moves_it_forward_by_exactly_the_span_given() {
    let clock = VirtualClock::starting_at(0);

    assert_eq!(clock.advance(250), 250);
    assert_eq!(clock.advance(750), 1_000);
    assert_eq!(clock.now_millis(), 1_000);
}

#[test]
fn both_contexts_read_one_instant() {
    // The property this type exists for: a roster ageing presence and a
    // conversation ageing a gap can never disagree about what time it is.
    let clock = Arc::new(VirtualClock::starting_at(10));
    let membership = Arc::clone(&clock) as Arc<dyn MembershipClock + Send + Sync>;
    let messaging = Arc::clone(&clock) as Arc<dyn MessagingClock + Send + Sync>;

    clock.advance(1_234);

    assert_eq!(membership.now().as_millis(), 1_244);
    assert_eq!(messaging.now().as_millis(), 1_244);
}

#[test]
fn advancing_to_a_past_instant_leaves_the_clock_where_it_is() {
    let clock = VirtualClock::starting_at(1_000);

    assert_eq!(clock.advance_to(400), 1_000);
    assert_eq!(clock.now_millis(), 1_000);
}

#[test]
fn advancing_to_a_future_instant_lands_exactly_on_it() {
    let clock = VirtualClock::starting_at(1_000);

    assert_eq!(clock.advance_to(4_200), 4_200);
}

#[test]
fn advancing_saturates_rather_than_wrapping_into_the_past() {
    let clock = VirtualClock::starting_at(u64::MAX - 5);

    assert_eq!(clock.advance(1_000), u64::MAX);
}
