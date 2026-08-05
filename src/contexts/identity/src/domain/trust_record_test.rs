use crate::domain::events::{PeerBlocked, PeerUnblocked, PeerVerified};
use crate::domain::{TrustRecord, TrustRecordError, VerificationState};
use crate::test_peers;

#[test]
fn a_new_record_is_unverified_and_not_blocked() {
    let record = TrustRecord::unverified(test_peers::bob());

    assert_eq!(record.peer(), test_peers::bob());
    assert_eq!(record.verification(), VerificationState::Unverified);
    assert!(!record.is_verified());
    assert!(!record.is_blocked());
}

#[test]
fn verifying_moves_unverified_to_verified_and_emits_the_event() {
    let mut record = TrustRecord::unverified(test_peers::bob());

    let event = record.verify();

    assert_eq!(
        event,
        Some(PeerVerified {
            peer: test_peers::bob()
        })
    );
    assert_eq!(record.verification(), VerificationState::Verified);
    assert!(record.is_verified());
}

#[test]
fn verifying_an_already_verified_peer_is_idempotent_and_emits_nothing() {
    let mut record = TrustRecord::unverified(test_peers::bob());
    record.verify().expect("first verification transitions");

    let event = record.verify();

    assert_eq!(event, None, "the transition already happened");
    assert_eq!(record.verification(), VerificationState::Verified);
}

#[test]
fn blocking_sets_the_flag_and_emits_the_event() {
    let mut record = TrustRecord::unverified(test_peers::bob());

    let event = record.block();

    assert_eq!(
        event,
        Ok(PeerBlocked {
            peer: test_peers::bob()
        })
    );
    assert!(record.is_blocked());
}

#[test]
fn blocking_an_already_blocked_peer_is_a_typed_error_and_changes_nothing() {
    let mut record = TrustRecord::unverified(test_peers::bob());
    record.block().expect("first block succeeds");

    let result = record.block();

    assert_eq!(result, Err(TrustRecordError::AlreadyBlocked));
    assert!(record.is_blocked());
}

#[test]
fn unblocking_clears_the_flag_and_emits_the_event() {
    let mut record = TrustRecord::unverified(test_peers::bob());
    record.block().expect("block succeeds");

    let event = record.unblock();

    assert_eq!(
        event,
        Ok(PeerUnblocked {
            peer: test_peers::bob()
        })
    );
    assert!(!record.is_blocked());
}

#[test]
fn unblocking_a_peer_that_is_not_blocked_is_a_typed_error_and_changes_nothing() {
    let mut record = TrustRecord::unverified(test_peers::bob());

    let result = record.unblock();

    assert_eq!(result, Err(TrustRecordError::NotBlocked));
    assert!(!record.is_blocked());
}

#[test]
fn blocking_a_verified_peer_preserves_verification_and_unblocking_restores_it() {
    let mut record = TrustRecord::unverified(test_peers::carol());
    record.verify().expect("verification transitions");

    record.block().expect("a verified peer can be blocked");
    assert!(record.is_blocked());
    assert!(
        record.is_verified(),
        "blocking must not discard an out-of-band verification"
    );

    record.unblock().expect("unblocking succeeds");
    assert!(!record.is_blocked());
    assert!(
        record.is_verified(),
        "unblocking restores the peer to its preserved verification state"
    );
}

#[test]
fn verification_and_blocking_are_orthogonal_in_both_orders() {
    let mut blocked_first = TrustRecord::unverified(test_peers::bob());
    blocked_first.block().expect("block succeeds");
    let event = blocked_first.verify();

    assert_eq!(
        event,
        Some(PeerVerified {
            peer: test_peers::bob()
        }),
        "a blocked peer's key can still be confirmed out-of-band"
    );
    assert!(blocked_first.is_verified());
    assert!(
        blocked_first.is_blocked(),
        "verifying must not silently unblock"
    );

    let mut verified_first = TrustRecord::unverified(test_peers::bob());
    verified_first.verify().expect("verification transitions");
    verified_first.block().expect("block succeeds");

    assert_eq!(
        (blocked_first.verification(), blocked_first.is_blocked()),
        (verified_first.verification(), verified_first.is_blocked()),
        "order of the two independent commands does not matter"
    );
}

#[test]
fn rehydrates_every_state_combination() {
    for verification in [VerificationState::Unverified, VerificationState::Verified] {
        for blocked in [false, true] {
            let record = TrustRecord::rehydrate(test_peers::alice(), verification, blocked);

            assert_eq!(record.peer(), test_peers::alice());
            assert_eq!(record.verification(), verification);
            assert_eq!(record.is_blocked(), blocked);
        }
    }
}

#[test]
fn records_are_scoped_to_one_peer() {
    let mut bob = TrustRecord::unverified(test_peers::bob());
    let carol = TrustRecord::unverified(test_peers::carol());

    bob.block().expect("block succeeds");

    assert!(bob.is_blocked());
    assert!(!carol.is_blocked(), "trust is per remote PeerId");
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (TrustRecordError::AlreadyBlocked, "peer is already blocked"),
        (TrustRecordError::NotBlocked, "peer is not blocked"),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
