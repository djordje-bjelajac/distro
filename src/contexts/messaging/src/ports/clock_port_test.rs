use crate::domain::Millis;
use crate::ports::ClockPort;
use crate::ports::port_fakes::FixedClock;

#[test]
fn the_port_is_object_safe_so_one_clock_can_be_shared() {
    let clock = FixedClock::at(Millis::from_millis(7));
    let port: &dyn ClockPort = &clock;

    assert_eq!(port.now(), Millis::from_millis(7));
}

#[test]
fn a_clock_reads_the_same_instant_until_it_is_advanced() {
    // AC13: domain and application tests advance time by hand and never wait.
    let clock = FixedClock::at(Millis::from_millis(1_000));

    assert_eq!(clock.now(), Millis::from_millis(1_000));
    assert_eq!(clock.now(), Millis::from_millis(1_000));

    clock.advance(500);

    assert_eq!(clock.now(), Millis::from_millis(1_500));
}

#[test]
fn successive_readings_never_go_backwards() {
    let clock = FixedClock::at(Millis::ZERO);
    let mut previous = clock.now();

    for step in 1..=5 {
        clock.advance(step);
        let now = clock.now();
        assert!(now > previous);
        previous = now;
    }
}
