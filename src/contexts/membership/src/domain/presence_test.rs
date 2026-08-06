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
            Presence::derive(Some(EVIDENCE_AT), at_age(age), windows),
            expected,
            "evidence age {age}ms"
        );
    }
}

#[test]
fn no_evidence_derives_unknown_at_every_instant() {
    // The question the old design had no answer to: what does a derivation over
    // accumulated evidence evaluate to *before any evidence exists*? Every
    // instant, from the origin to the far end of the timeline, must give the
    // same answer, because no time has passed for a peer that never spoke.
    let windows = LivenessWindows::DEFAULT;

    for now in [
        Millis::ZERO,
        Millis::from_millis(1),
        EVIDENCE_AT,
        at_age(windows.offline().as_millis()),
        at_age(u64::from(u32::MAX)),
        Millis::MAX,
    ] {
        assert_eq!(
            Presence::derive(None, now, windows),
            Presence::Unknown,
            "no evidence, read at {now}"
        );
    }
}

#[test]
fn unknown_is_a_distinct_verdict_and_never_offline() {
    // `Offline` is the negative claim "treat the peer as gone"; `Unknown` is
    // the absence of a measurement. Collapsing them would put a false departure
    // on most rows of most screens after every cache load — and would let
    // `PeerPresenceExpired` fire with no evidence instant to report.
    let windows = LivenessWindows::DEFAULT;
    let unknown = Presence::derive(None, at_age(u64::from(u32::MAX)), windows);
    let offline = Presence::derive(Some(EVIDENCE_AT), at_age(u64::from(u32::MAX)), windows);

    assert_eq!(unknown, Presence::Unknown);
    assert_eq!(offline, Presence::Offline);
    assert_ne!(unknown, offline);
    assert!(!unknown.is_offline());
    assert!(!unknown.is_online());
}

#[test]
fn evidence_from_the_future_counts_as_fresh_rather_than_wrapping() {
    let before_the_evidence = Millis::from_millis(EVIDENCE_AT.as_millis() - 5_000);

    assert_eq!(
        Presence::derive(
            Some(EVIDENCE_AT),
            before_the_evidence,
            LivenessWindows::DEFAULT
        ),
        Presence::Online
    );
}

#[test]
fn custom_windows_are_honoured() {
    let windows = LivenessWindows::new(DurationMillis::from_secs(1), DurationMillis::from_secs(2))
        .expect("ordered windows");

    assert_eq!(
        Presence::derive(Some(EVIDENCE_AT), at_age(999), windows),
        Presence::Online
    );
    assert_eq!(
        Presence::derive(Some(EVIDENCE_AT), at_age(1_000), windows),
        Presence::Stale
    );
    assert_eq!(
        Presence::derive(Some(EVIDENCE_AT), at_age(2_000), windows),
        Presence::Offline
    );
    assert_eq!(
        Presence::derive(None, at_age(2_000), windows),
        Presence::Unknown,
        "no window makes an absent measurement into a verdict"
    );
}

#[test]
fn derivation_is_a_pure_function_of_its_inputs() {
    let now = at_age(45_000);
    let first = Presence::derive(Some(EVIDENCE_AT), now, LivenessWindows::DEFAULT);
    let second = Presence::derive(Some(EVIDENCE_AT), now, LivenessWindows::DEFAULT);

    assert_eq!(first, second);
}

#[test]
fn classifies_itself_for_callers_that_only_care_about_reachability() {
    assert!(Presence::Online.is_online());
    assert!(!Presence::Stale.is_online());
    assert!(!Presence::Offline.is_online());
    assert!(!Presence::Unknown.is_online());

    assert!(Presence::Offline.is_offline());
    assert!(!Presence::Stale.is_offline());
    assert!(!Presence::Online.is_offline());
    assert!(!Presence::Unknown.is_offline());

    assert!(Presence::Unknown.is_unknown());
    assert!(!Presence::Online.is_unknown());
    assert!(!Presence::Stale.is_unknown());
    assert!(!Presence::Offline.is_unknown());
}

#[test]
fn displays_a_label_for_the_roster_pane() {
    assert_eq!(Presence::Online.to_string(), "online");
    assert_eq!(Presence::Stale.to_string(), "stale");
    assert_eq!(Presence::Offline.to_string(), "offline");
    assert_eq!(Presence::Unknown.to_string(), "unknown");
}
