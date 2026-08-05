use std::error::Error as _;

use crate::domain::TrustRecordError;
use crate::ports::{PeerTrustCommandError, TrustRecordStoreError};

#[test]
fn a_domain_rejection_and_a_store_failure_stay_distinguishable() {
    let rejected = PeerTrustCommandError::from(TrustRecordError::AlreadyBlocked);
    let store = PeerTrustCommandError::from(TrustRecordStoreError::WriteFailed);

    assert_eq!(
        rejected,
        PeerTrustCommandError::Rejected(TrustRecordError::AlreadyBlocked)
    );
    assert_eq!(
        store,
        PeerTrustCommandError::Store(TrustRecordStoreError::WriteFailed)
    );
    assert_ne!(
        rejected, store,
        "\"the command would change nothing\" is not \"the change may not have survived\""
    );
}

#[test]
fn each_variant_displays_its_cause_and_exposes_it_as_the_source() {
    let cases = [
        (
            PeerTrustCommandError::Rejected(TrustRecordError::NotBlocked),
            "peer is not blocked",
        ),
        (
            PeerTrustCommandError::Store(TrustRecordStoreError::Unreadable),
            "trust record store could not be read",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert!(error.source().is_some());
        let _: &dyn std::error::Error = &error;
    }
}
