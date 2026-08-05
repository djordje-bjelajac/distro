use crate::domain::{Session, SessionDirection, SessionError, SessionState};
use crate::test_peers;

#[test]
fn opens_in_the_connecting_state_with_its_direction_and_remote() {
    let session = Session::open(
        test_peers::alice(),
        test_peers::bob(),
        SessionDirection::Outbound,
    )
    .expect("a session to another peer is legal");

    assert_eq!(session.remote(), test_peers::bob());
    assert_eq!(session.direction(), SessionDirection::Outbound);
    assert_eq!(session.state(), SessionState::Connecting);
    assert!(session.is_live());
    assert!(!session.is_established());
}

#[test]
fn rejects_a_session_whose_remote_is_the_local_peer() {
    // Invariant 2. Both directions, because a self-dial and a self-inbound are
    // the same mistake seen from two sides.
    for direction in [SessionDirection::Outbound, SessionDirection::Inbound] {
        assert_eq!(
            Session::open(test_peers::alice(), test_peers::alice(), direction),
            Err(SessionError::SelfConnection)
        );
    }
}

#[test]
fn establish_moves_connecting_to_established() {
    let mut session = Session::open(
        test_peers::alice(),
        test_peers::bob(),
        SessionDirection::Inbound,
    )
    .unwrap();

    assert_eq!(session.establish(), Ok(()));
    assert_eq!(session.state(), SessionState::Established);
    assert!(session.is_established());
    assert!(session.is_live());
}

#[test]
fn establish_is_rejected_once_already_established() {
    let mut session = Session::open(
        test_peers::alice(),
        test_peers::bob(),
        SessionDirection::Outbound,
    )
    .unwrap();
    session.establish().unwrap();

    assert_eq!(
        session.establish(),
        Err(SessionError::InvalidTransition {
            from: SessionState::Established,
            to: SessionState::Established,
        })
    );
}

#[test]
fn establish_is_rejected_after_close() {
    // Closed is terminal: a link that dropped is never revived in place, the
    // application opens a new session instead.
    let mut session = Session::open(
        test_peers::alice(),
        test_peers::bob(),
        SessionDirection::Outbound,
    )
    .unwrap();
    session.close().unwrap();

    assert_eq!(
        session.establish(),
        Err(SessionError::InvalidTransition {
            from: SessionState::Closed,
            to: SessionState::Established,
        })
    );
}

#[test]
fn close_ends_a_session_from_either_live_state() {
    for establish_first in [false, true] {
        let mut session = Session::open(
            test_peers::alice(),
            test_peers::bob(),
            SessionDirection::Outbound,
        )
        .unwrap();
        if establish_first {
            session.establish().unwrap();
        }

        assert_eq!(session.close(), Ok(()));
        assert_eq!(session.state(), SessionState::Closed);
        assert!(!session.is_live());
        assert!(!session.is_established());
    }
}

#[test]
fn close_is_rejected_a_second_time() {
    let mut session = Session::open(
        test_peers::alice(),
        test_peers::bob(),
        SessionDirection::Outbound,
    )
    .unwrap();
    session.close().unwrap();

    assert_eq!(
        session.close(),
        Err(SessionError::InvalidTransition {
            from: SessionState::Closed,
            to: SessionState::Closed,
        })
    );
}

#[test]
fn only_connecting_and_established_count_as_live() {
    assert!(SessionState::Connecting.is_live());
    assert!(SessionState::Established.is_live());
    assert!(!SessionState::Closed.is_live());
}

#[test]
fn a_directions_initiator_is_the_peer_that_dialled() {
    let local = test_peers::alice();
    let remote = test_peers::bob();

    assert_eq!(SessionDirection::Outbound.initiator(local, remote), local);
    assert_eq!(SessionDirection::Inbound.initiator(local, remote), remote);
}

#[test]
fn directions_are_opposites_of_each_other() {
    assert_eq!(
        SessionDirection::Outbound.opposite(),
        SessionDirection::Inbound
    );
    assert_eq!(
        SessionDirection::Inbound.opposite(),
        SessionDirection::Outbound
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            SessionError::SelfConnection,
            "a session cannot be opened to the local peer itself",
        ),
        (
            SessionError::InvalidTransition {
                from: SessionState::Closed,
                to: SessionState::Established,
            },
            "session cannot move from closed to established",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
