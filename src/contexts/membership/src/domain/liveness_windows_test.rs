use crate::domain::{DurationMillis, LivenessWindows, LivenessWindowsError};

#[test]
fn the_default_windows_leave_a_stale_band_between_online_and_offline() {
    let windows = LivenessWindows::DEFAULT;

    assert!(
        windows.online() < windows.offline(),
        "without a gap a peer would jump straight from Online to Offline"
    );
}

#[test]
fn the_default_windows_are_multiples_of_the_assumed_heartbeat_interval() {
    let heartbeat = LivenessWindows::HEARTBEAT_INTERVAL.as_millis();

    assert_eq!(
        LivenessWindows::DEFAULT.online().as_millis(),
        3 * heartbeat,
        "one lost heartbeat plus jitter must not flip a peer out of Online"
    );
    assert_eq!(
        LivenessWindows::DEFAULT.offline().as_millis(),
        6 * heartbeat
    );
}

#[test]
fn default_trait_agrees_with_the_default_constant() {
    assert_eq!(LivenessWindows::default(), LivenessWindows::DEFAULT);
}

#[test]
fn custom_windows_are_accepted_when_online_precedes_offline() {
    let windows = LivenessWindows::new(DurationMillis::from_secs(1), DurationMillis::from_secs(2))
        .expect("an ordered pair of windows is legal");

    assert_eq!(windows.online(), DurationMillis::from_secs(1));
    assert_eq!(windows.offline(), DurationMillis::from_secs(2));
}

#[test]
fn rejects_windows_that_are_not_strictly_ordered() {
    let equal = LivenessWindows::new(DurationMillis::from_secs(5), DurationMillis::from_secs(5));
    let inverted = LivenessWindows::new(DurationMillis::from_secs(9), DurationMillis::from_secs(5));

    assert_eq!(equal, Err(LivenessWindowsError::WindowsNotOrdered));
    assert_eq!(inverted, Err(LivenessWindowsError::WindowsNotOrdered));
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let error = LivenessWindowsError::WindowsNotOrdered;

    assert_eq!(
        error.to_string(),
        "online liveness window must be shorter than the offline window"
    );
    let _: &dyn std::error::Error = &error;
}
