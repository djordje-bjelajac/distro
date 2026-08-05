use shared_types::PeerId;

use crate::domain::{
    Session, SessionCollapse, SessionCollapseError, SessionDirection, SessionState,
};
use crate::test_peers;

/// Every unordered pair of the fixed key fixtures, deterministic and free of
/// any RNG. Five keys give ten pairs spanning both possible orderings of the
/// lexicographic rule.
fn peer_pairs() -> Vec<(PeerId, PeerId)> {
    let peers = test_peers::all();
    let mut pairs = Vec::new();

    for (index, first) in peers.iter().enumerate() {
        for second in &peers[index + 1..] {
            pairs.push((*first, *second));
        }
    }

    pairs
}

#[test]
fn the_fixtures_span_both_orderings() {
    let pairs = peer_pairs();

    assert_eq!(pairs.len(), 10);
    assert!(
        pairs.iter().any(|(first, second)| first < second),
        "fixtures must contain an ascending pair"
    );
    assert!(
        pairs.iter().any(|(first, second)| first > second),
        "fixtures must contain a descending pair"
    );
}

#[test]
fn the_session_initiated_by_the_lower_peer_id_survives() {
    // Invariant 3, stated directly: whichever side asks, the survivor is the
    // session the lexicographically lower PeerId initiated.
    for (first, second) in peer_pairs() {
        for (local, remote) in [(first, second), (second, first)] {
            let collapse = SessionCollapse::resolve(local, remote).expect("distinct peers");

            let lower = if local < remote { local } else { remote };
            assert_eq!(collapse.initiator(), lower);
            assert_eq!(
                collapse.survivor().initiator(local, remote),
                lower,
                "the surviving direction must be the lower peer's own dial"
            );
        }
    }
}

#[test]
fn both_sides_of_a_pair_compute_the_same_outcome() {
    // The rule is only useful if it is symmetric: A keeping its outbound
    // session must be the same wire session B keeps as its inbound one, with
    // no message exchanged to agree on it.
    for (first, second) in peer_pairs() {
        let at_first = SessionCollapse::resolve(first, second).expect("distinct peers");
        let at_second = SessionCollapse::resolve(second, first).expect("distinct peers");

        assert_eq!(
            at_first.initiator(),
            at_second.initiator(),
            "both sides must name the same initiator"
        );
        assert_eq!(
            at_first.survivor(),
            at_second.survivor().opposite(),
            "one side's outbound survivor is the other side's inbound survivor"
        );
        assert_eq!(
            at_first.superseded(),
            at_second.superseded().opposite(),
            "and likewise for the session both sides drop"
        );
    }
}

#[test]
fn the_superseded_direction_is_the_opposite_of_the_survivor() {
    for (first, second) in peer_pairs() {
        let collapse = SessionCollapse::resolve(first, second).unwrap();

        assert_eq!(collapse.superseded(), collapse.survivor().opposite());
    }
}

#[test]
fn resolving_against_the_local_peer_itself_is_rejected() {
    // Invariant 2 again, at the collapse boundary: there is no "lower" peer
    // when both are the same key, and the pair should never have existed.
    assert_eq!(
        SessionCollapse::resolve(test_peers::alice(), test_peers::alice()),
        Err(SessionCollapseError::SelfConnection)
    );
}

#[test]
fn between_two_live_opposite_sessions_agrees_with_resolve() {
    let local = test_peers::alice();
    let remote = test_peers::bob();
    let outbound = Session::open(local, remote, SessionDirection::Outbound).unwrap();
    let inbound = Session::open(local, remote, SessionDirection::Inbound).unwrap();

    let collapse = SessionCollapse::between(local, &outbound, &inbound).expect("a legal pair");

    assert_eq!(collapse, SessionCollapse::resolve(local, remote).unwrap());
}

#[test]
fn between_accepts_the_two_sessions_in_either_argument_order() {
    let local = test_peers::alice();
    let remote = test_peers::bob();
    let outbound = Session::open(local, remote, SessionDirection::Outbound).unwrap();
    let inbound = Session::open(local, remote, SessionDirection::Inbound).unwrap();

    assert_eq!(
        SessionCollapse::between(local, &outbound, &inbound),
        SessionCollapse::between(local, &inbound, &outbound)
    );
}

#[test]
fn between_rejects_two_sessions_in_the_same_direction() {
    let local = test_peers::alice();
    let remote = test_peers::bob();
    let first = Session::open(local, remote, SessionDirection::Outbound).unwrap();
    let second = Session::open(local, remote, SessionDirection::Outbound).unwrap();

    assert_eq!(
        SessionCollapse::between(local, &first, &second),
        Err(SessionCollapseError::SameDirection)
    );
}

#[test]
fn between_rejects_sessions_that_are_not_with_the_same_remote() {
    let local = test_peers::alice();
    let outbound = Session::open(local, test_peers::bob(), SessionDirection::Outbound).unwrap();
    let inbound = Session::open(local, test_peers::carol(), SessionDirection::Inbound).unwrap();

    assert_eq!(
        SessionCollapse::between(local, &outbound, &inbound),
        Err(SessionCollapseError::RemoteMismatch)
    );
}

#[test]
fn between_rejects_a_session_that_is_no_longer_live() {
    // Nothing collapses against a closed session: there is only one live
    // session left, which is the ordinary case and not a simultaneous connect.
    let local = test_peers::alice();
    let remote = test_peers::bob();
    let mut outbound = Session::open(local, remote, SessionDirection::Outbound).unwrap();
    let inbound = Session::open(local, remote, SessionDirection::Inbound).unwrap();
    outbound.close().unwrap();

    assert_eq!(
        SessionCollapse::between(local, &outbound, &inbound),
        Err(SessionCollapseError::SessionNotLive {
            state: SessionState::Closed
        })
    );
}

#[test]
fn between_rejects_a_pair_whose_remote_is_the_local_peer() {
    // Unreachable through `Session::open`, but `between` is a public rule and
    // must not depend on its caller having used the constructor.
    let local = test_peers::alice();
    let outbound = Session::open(local, test_peers::bob(), SessionDirection::Outbound).unwrap();
    let inbound = Session::open(local, test_peers::bob(), SessionDirection::Inbound).unwrap();

    assert_eq!(
        SessionCollapse::between(test_peers::bob(), &outbound, &inbound),
        Err(SessionCollapseError::SelfConnection)
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            SessionCollapseError::SelfConnection,
            "a session pair cannot be collapsed against the local peer itself",
        ),
        (
            SessionCollapseError::RemoteMismatch,
            "the two sessions are not with the same remote peer",
        ),
        (
            SessionCollapseError::SameDirection,
            "the two sessions have the same direction, so no simultaneous connect occurred",
        ),
        (
            SessionCollapseError::SessionNotLive {
                state: SessionState::Closed,
            },
            "a session in state closed cannot take part in a collapse",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
