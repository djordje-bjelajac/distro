use crate::domain::{JoinTicketError, Millis};
use crate::ports::{
    BootstrapAttempt, BootstrapRung, JoinDiagnostic, PeerCacheError, PeerDiscoveryError,
    PeerTransportError, RungFailure,
};
use crate::test_peers;

/// The shape AC3 is about: every rung reached, every one of them failed.
fn every_rung_failed() -> JoinDiagnostic {
    JoinDiagnostic {
        attempts: vec![
            BootstrapAttempt::failed(
                BootstrapRung::CachedPeers,
                RungFailure::Cache(PeerCacheError::Unreadable),
            ),
            BootstrapAttempt::failed(
                BootstrapRung::LocalNetwork,
                RungFailure::Unreachable { candidates: 2 },
            ),
            BootstrapAttempt::failed(
                BootstrapRung::JoinTicket,
                RungFailure::Ticket(JoinTicketError::Expired {
                    expires_at: Millis::from_millis(5_000),
                    now: Millis::from_millis(9_000),
                }),
            ),
        ],
        listen_failure: None,
        announce_failure: None,
    }
}

#[test]
fn a_walk_that_connected_nothing_names_every_rung_and_its_reason() {
    let diagnostic = every_rung_failed();

    let rendered = diagnostic.to_string();

    for rung in BootstrapRung::LADDER {
        assert!(
            rendered.contains(&rung.to_string()),
            "AC3: the diagnostic must name what was tried; {rung} is missing from:\n{rendered}"
        );
    }
    assert!(rendered.contains("peer cache could not be read"));
    assert!(rendered.contains("2 peers tried, none answered"));
    assert!(
        rendered.contains("expired"),
        "an expired ticket is the one failure the user can act on immediately"
    );
}

#[test]
fn a_failed_walk_is_reported_as_a_failure_not_as_silence() {
    let rendered = every_rung_failed().to_string();

    assert!(rendered.starts_with("could not reach the network"));
    assert!(!every_rung_failed().succeeded());
    assert_eq!(every_rung_failed().connected_peer(), None);
}

#[test]
fn a_successful_walk_names_the_peer_that_answered_and_stops_there() {
    let diagnostic = JoinDiagnostic {
        attempts: vec![BootstrapAttempt::connected(
            BootstrapRung::CachedPeers,
            test_peers::bob(),
        )],
        ..JoinDiagnostic::default()
    };

    assert!(diagnostic.succeeded());
    assert_eq!(diagnostic.connected_peer(), Some(test_peers::bob()));
    assert_eq!(
        diagnostic.rungs_tried(),
        vec![BootstrapRung::CachedPeers],
        "a rung that connects ends the walk; the costlier rungs are never reached"
    );
    assert!(diagnostic.to_string().starts_with("joined the network"));
}

#[test]
fn each_rungs_reason_is_readable_on_its_own() {
    let diagnostic = every_rung_failed();

    assert_eq!(
        diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::Cache(PeerCacheError::Unreadable))
    );
    assert_eq!(
        diagnostic.failure_of(BootstrapRung::LocalNetwork),
        Some(RungFailure::Unreachable { candidates: 2 })
    );
    assert!(matches!(
        diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::Ticket(JoinTicketError::Expired { .. }))
    ));
}

#[test]
fn a_rung_that_was_never_reached_reports_no_failure() {
    let diagnostic = JoinDiagnostic {
        attempts: vec![BootstrapAttempt::connected(
            BootstrapRung::CachedPeers,
            test_peers::bob(),
        )],
        ..JoinDiagnostic::default()
    };

    assert_eq!(diagnostic.failure_of(BootstrapRung::JoinTicket), None);
    assert_eq!(diagnostic.failure_of(BootstrapRung::CachedPeers), None);
}

#[test]
fn being_unreachable_or_unannounced_is_reported_even_when_the_join_worked() {
    // Joining while unable to listen means every link this peer has, it made
    // itself. That is a working join and a silent half-failure, so it is
    // stated rather than inferred from a peer count that never grows.
    let diagnostic = JoinDiagnostic {
        attempts: vec![BootstrapAttempt::connected(
            BootstrapRung::LocalNetwork,
            test_peers::bob(),
        )],
        listen_failure: Some(PeerTransportError::ListenFailed),
        announce_failure: Some(PeerDiscoveryError::AnnouncementRejected),
    };

    let rendered = diagnostic.to_string();

    assert!(diagnostic.succeeded());
    assert!(rendered.contains("not listening"));
    assert!(rendered.contains("not announced"));
}

#[test]
fn an_empty_diagnostic_still_says_something() {
    // The degenerate case a caller could hit by reading the outcome of a join
    // that never ran. It must not render as an empty string.
    assert_eq!(
        JoinDiagnostic::default().to_string(),
        "could not reach the network; every bootstrap path failed"
    );
}
