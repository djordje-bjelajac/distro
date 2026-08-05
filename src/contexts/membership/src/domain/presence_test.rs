use crate::domain::{DurationMillis, LivenessWindows, Millis, Presence};

/// Evidence recorded well away from the origin, so that "age" cannot be
/// confused with "absolute reading".
const EVIDENCE_AT: Millis = Millis::from_millis(1_000_000);

fn at_age(age_millis: u64) -> Millis {
    EVIDENCE_AT.saturating_add(DurationMillis::from_millis(age_millis))
}

#[test]
fn derivation_truth_table_over_the_default_windows() {
    let windows = LivenessWindows::DEFAULT;
    let online = windows.online().as_millis();
    let offline = windows.offline().as_millis();

    // Exact boundary values at each window edge: the windows are half-open, so
    // a peer is Online while its evidence age is *strictly* below the online
    // window, and Offline from the offline window onwards.
    let table = [
        (0, Presence::Online),
        (1, Presence::Online),
        (online - 1, Presence::Online),
        (online, Presence::Stale),
        (online + 1, Presence::Stale),
        (offline - 1, Presence::Stale),
        (offline, Presence::Offline),
        (offline + 1, Presence::Offline),
        (u64::from(u32::MAX), Presence::Offline),
    ];

    for (age, expected) in table {
        assert_eq!(
            Presence::derive(EVIDENCE_AT, at_age(age), windows),
            expected,
            "evidence age {age}ms"
        );
    }
}

#[test]
fn evidence_from_the_future_counts_as_fresh_rather_than_wrapping() {
    let before_the_evidence = Millis::from_millis(EVIDENCE_AT.as_millis() - 5_000);

    assert_eq!(
        Presence::derive(EVIDENCE_AT, before_the_evidence, LivenessWindows::DEFAULT),
        Presence::Online
    );
}

#[test]
fn custom_windows_are_honoured() {
    let windows = LivenessWindows::new(DurationMillis::from_secs(1), DurationMillis::from_secs(2))
        .expect("ordered windows");

    assert_eq!(
        Presence::derive(EVIDENCE_AT, at_age(999), windows),
        Presence::Online
    );
    assert_eq!(
        Presence::derive(EVIDENCE_AT, at_age(1_000), windows),
        Presence::Stale
    );
    assert_eq!(
        Presence::derive(EVIDENCE_AT, at_age(2_000), windows),
        Presence::Offline
    );
}

#[test]
fn derivation_is_a_pure_function_of_its_inputs() {
    let now = at_age(45_000);
    let first = Presence::derive(EVIDENCE_AT, now, LivenessWindows::DEFAULT);
    let second = Presence::derive(EVIDENCE_AT, now, LivenessWindows::DEFAULT);

    assert_eq!(first, second);
}

#[test]
fn classifies_itself_for_callers_that_only_care_about_reachability() {
    assert!(Presence::Online.is_online());
    assert!(!Presence::Stale.is_online());
    assert!(!Presence::Offline.is_online());

    assert!(Presence::Offline.is_offline());
    assert!(!Presence::Stale.is_offline());
    assert!(!Presence::Online.is_offline());
}

#[test]
fn displays_a_label_for_the_roster_pane() {
    assert_eq!(Presence::Online.to_string(), "online");
    assert_eq!(Presence::Stale.to_string(), "stale");
    assert_eq!(Presence::Offline.to_string(), "offline");
}
