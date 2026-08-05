use membership::domain::LivenessWindows;
use messaging::domain::Conversation;

use crate::runtime::TickSchedule;

#[test]
fn everything_is_due_on_the_first_pass() {
    // The first heartbeat is what tells the network this peer exists.
    let mut schedule = TickSchedule::starting_at(0);

    let due = schedule.due(0);

    assert!(due.presence);
    assert!(due.gaps);
    assert!(due.trust);
    assert!(due.any());
}

#[test]
fn nothing_is_due_again_immediately() {
    let mut schedule = TickSchedule::with_intervals(0, 100, 20, 50);
    schedule.due(0);

    let due = schedule.due(1);

    assert!(!due.any());
}

#[test]
fn each_duty_comes_due_on_its_own_interval() {
    let mut schedule = TickSchedule::with_intervals(0, 100, 20, 50);
    schedule.due(0);

    assert_eq!(
        (schedule.due(20).gaps, schedule.due(20).gaps),
        (true, false),
        "the gap sweep is due at its interval and not twice"
    );
    assert!(!schedule.due(40).presence);
    assert!(schedule.due(50).trust);
    assert!(schedule.due(100).presence);
}

#[test]
fn a_long_stall_performs_each_duty_once_rather_than_accumulating() {
    // A laptop that slept for an hour must not spend the first seconds after
    // waking re-sweeping the same conversations. Every duty is idempotent, so
    // once is exactly right.
    let mut schedule = TickSchedule::with_intervals(0, 100, 20, 50);
    schedule.due(0);

    assert!(schedule.due(1_000_000).any());
    assert!(!schedule.due(1_000_001).any());
}

#[test]
fn presence_and_heartbeat_share_the_cadence_the_windows_are_derived_from() {
    // The windows say `Online` for three missed heartbeats and `Offline` after
    // six. That is only true if something beats at the interval they are
    // derived from.
    assert_eq!(
        TickSchedule::PRESENCE_INTERVAL_MILLIS,
        LivenessWindows::HEARTBEAT_INTERVAL.as_millis()
    );
}

#[test]
fn the_gap_sweep_runs_several_times_per_tolerance_window() {
    // Sweeping once per window would let a gap live up to twice its tolerance.
    let interval = TickSchedule::GAP_INTERVAL_MILLIS;
    let window = Conversation::GAP_TOLERANCE.as_millis();

    assert!(interval > 0, "an interval of zero is a busy loop");
    assert!(interval < window, "{interval} is not shorter than {window}");
}

#[test]
fn a_zero_interval_is_refused_into_one_millisecond() {
    // An interval of zero is a busy loop, never what a caller meant.
    let mut schedule = TickSchedule::with_intervals(0, 0, 0, 0);
    schedule.due(0);

    assert!(!schedule.due(0).any());
    assert!(schedule.due(1).any());
}
