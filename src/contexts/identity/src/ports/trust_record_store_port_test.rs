use crate::domain::{TrustRecord, VerificationState};
use crate::ports::port_fakes::{InMemoryTrustRecordStore, UnusableTrustRecordStore};
use crate::ports::{TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

#[test]
fn an_unknown_peer_reads_as_absent_rather_than_as_an_error() {
    let store = InMemoryTrustRecordStore::empty();
    let port: &dyn TrustRecordStorePort = &store;

    assert_eq!(
        port.load_trust_record(test_peers::alice()),
        Ok(None),
        "trust-on-first-use: never having seen a peer is not a failure"
    );
}

#[test]
fn saving_a_record_twice_upserts_rather_than_duplicating_the_peer() {
    let store = InMemoryTrustRecordStore::empty();
    let port: &dyn TrustRecordStorePort = &store;
    let mut record = TrustRecord::unverified(test_peers::alice());

    port.save_trust_record(&record).expect("first save");
    record.verify();
    port.save_trust_record(&record).expect("second save");

    let loaded = port
        .load_trust_record(test_peers::alice())
        .expect("load")
        .expect("record exists");
    assert_eq!(loaded.verification(), VerificationState::Verified);
    assert_eq!(port.list_blocked_peers().expect("list"), Vec::new());
}

#[test]
fn the_block_list_holds_exactly_the_peers_whose_flag_is_set() {
    let mut blocked = TrustRecord::unverified(test_peers::bob());
    blocked.block().expect("bob is not blocked yet");
    let store = InMemoryTrustRecordStore::seeded_with([
        TrustRecord::unverified(test_peers::alice()),
        blocked,
        TrustRecord::rehydrate(test_peers::carol(), VerificationState::Verified, true),
    ]);
    let port: &dyn TrustRecordStorePort = &store;

    assert_eq!(
        port.list_blocked_peers().expect("list"),
        vec![test_peers::bob(), test_peers::carol()],
        "a verified peer can still be blocked: the two axes are orthogonal"
    );
}

#[test]
fn reports_every_failure_as_a_typed_error_rather_than_a_panic() {
    let failures = [
        TrustRecordStoreError::Unreadable,
        TrustRecordStoreError::Corrupt,
        TrustRecordStoreError::UnsupportedSchemaVersion { found: 2 },
        TrustRecordStoreError::WriteFailed,
    ];

    for failure in failures {
        let store = UnusableTrustRecordStore(failure);
        let port: &dyn TrustRecordStorePort = &store;

        assert_eq!(port.load_trust_record(test_peers::alice()), Err(failure));
        assert_eq!(
            port.save_trust_record(&TrustRecord::unverified(test_peers::alice())),
            Err(failure)
        );
        assert_eq!(port.list_blocked_peers(), Err(failure));
    }
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            TrustRecordStoreError::Unreadable,
            "trust record store could not be read",
        ),
        (
            TrustRecordStoreError::Corrupt,
            "trust record store does not contain usable records",
        ),
        (
            TrustRecordStoreError::UnsupportedSchemaVersion { found: 7 },
            "trust record store has unsupported schema version 7",
        ),
        (
            TrustRecordStoreError::WriteFailed,
            "trust record could not be written",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
