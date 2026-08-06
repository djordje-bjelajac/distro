use shared_types::PeerDisconnected;

use crate::ports::{EventPublisherError, ForgetPeersError, ForgetPeersOutcome, PeerCacheError};
use crate::test_peers;

/// The shape the whole type exists for: peers genuinely forgotten *and* a
/// cache that refused, at once. If these two could not be stated together the
/// interface would have to pick one of them to tell the user, and either
/// choice is a lie.
#[test]
fn a_forget_can_report_peers_gone_and_a_cache_that_refused_at_the_same_time() {
    let outcome = ForgetPeersOutcome {
        forgotten: 12,
        disconnected: vec![PeerDisconnected {
            peer: test_peers::bob(),
        }],
        cache_failure: Some(PeerCacheError::WriteFailed),
    };

    assert_eq!(outcome.forgotten, 12);
    assert_eq!(outcome.cache_failure, Some(PeerCacheError::WriteFailed));
    assert_eq!(outcome.disconnected.len(), 1);
}

#[test]
fn a_forget_that_closed_nothing_is_representable() {
    let outcome = ForgetPeersOutcome {
        forgotten: 0,
        disconnected: Vec::new(),
        cache_failure: None,
    };

    assert_eq!(outcome.forgotten, 0);
    assert!(outcome.disconnected.is_empty());
    assert_eq!(outcome.cache_failure, None);
}

#[test]
fn every_refusal_says_why_in_words_a_user_can_act_on() {
    let cases = [
        (
            ForgetPeersError::JoinInFlight,
            "peers cannot be forgotten while a join is in flight",
        ),
        (
            ForgetPeersError::Publisher(EventPublisherError::Unavailable),
            "the event publisher is not available",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}

#[test]
fn a_publisher_failure_converts_so_a_handler_can_use_the_question_mark() {
    assert_eq!(
        ForgetPeersError::from(EventPublisherError::Unavailable),
        ForgetPeersError::Publisher(EventPublisherError::Unavailable)
    );
}
