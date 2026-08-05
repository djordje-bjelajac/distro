use crate::domain::{DurationMillis, Millis};
use crate::ports::ClockPort;
use crate::ports::port_fakes::ManualClock;

#[test]
fn reads_this_contexts_own_instant_type_through_a_trait_object() {
    let clock = ManualClock::starting_at(Millis::from_millis(5_000));
    let port: &dyn ClockPort = &clock;

    assert_eq!(port.now(), Millis::from_millis(5_000));
}

#[test]
fn successive_readings_never_go_backwards() {
    // The contract every time-dependent rule in this context leans on (D11):
    // presence ages and ticket expiry are only meaningful against a monotonic
    // source.
    let clock = ManualClock::starting_at(Millis::ZERO);
    let port: &dyn ClockPort = &clock;

    let first = port.now();
    clock.advance(DurationMillis::from_secs(1));
    let second = port.now();

    assert!(second > first);
}

#[test]
fn a_test_clock_makes_domain_tests_free_of_real_time() {
    // AC13: nothing in domain or application tests may consult a real clock.
    let clock = ManualClock::starting_at(Millis::ZERO);
    clock.advance(DurationMillis::from_secs(3_600));

    assert_eq!(clock.now(), Millis::from_millis(3_600_000));
}
