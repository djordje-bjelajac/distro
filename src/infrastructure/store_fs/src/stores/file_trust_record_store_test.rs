use std::fs;
use std::path::PathBuf;

use identity::domain::{TrustRecord, VerificationState};
use identity::ports::{TrustRecordStoreError, TrustRecordStorePort};

use crate::format::hex_bytes;
use crate::stores::FileTrustRecordStore;
use crate::test_dir::TestDir;
use crate::test_peers::{alice, bob, carol};

fn store(dir: &TestDir) -> FileTrustRecordStore {
    FileTrustRecordStore::at(dir.file(FileTrustRecordStore::FILE_NAME))
}

fn plant(dir: &TestDir, contents: &str) -> PathBuf {
    let path = dir.file(FileTrustRecordStore::FILE_NAME);
    fs::write(&path, contents).expect("the plant must land");
    path
}

#[test]
fn an_absent_store_yields_no_record_and_no_error() {
    let dir = TestDir::new("trust-absent-file");
    let store = store(&dir);

    // Trust-on-first-use: an unknown peer is the starting point, not a failure.
    assert_eq!(store.load_trust_record(alice()), Ok(None));
    assert_eq!(store.list_blocked_peers(), Ok(Vec::new()));
}

#[test]
fn an_unknown_peer_in_a_populated_store_is_still_none() {
    let dir = TestDir::new("trust-unknown-peer");
    let store = store(&dir);

    store
        .save_trust_record(&TrustRecord::unverified(alice()))
        .expect("the save must land");

    assert_eq!(store.load_trust_record(bob()), Ok(None));
}

#[test]
fn every_combination_of_the_two_axes_round_trips() {
    let dir = TestDir::new("trust-round-trip");
    let store = store(&dir);

    for (peer, verification, blocked) in [
        (alice(), VerificationState::Unverified, false),
        (bob(), VerificationState::Verified, false),
        (carol(), VerificationState::Verified, true),
    ] {
        let record = TrustRecord::rehydrate(peer, verification, blocked);
        store
            .save_trust_record(&record)
            .expect("the save must land");

        assert_eq!(store.load_trust_record(peer), Ok(Some(record)));
    }

    // Verification and blocking are orthogonal, so a blocked-and-verified peer
    // must come back with both facts intact.
    let carol_record = store
        .load_trust_record(carol())
        .expect("the load must succeed")
        .expect("carol has a record");
    assert!(carol_record.is_verified() && carol_record.is_blocked());
}

#[test]
fn saving_the_same_peer_again_replaces_its_record() {
    let dir = TestDir::new("trust-upsert");
    let store = store(&dir);

    store
        .save_trust_record(&TrustRecord::unverified(alice()))
        .expect("the save must land");
    store
        .save_trust_record(&TrustRecord::rehydrate(
            alice(),
            VerificationState::Verified,
            true,
        ))
        .expect("the save must land");

    let contents =
        fs::read_to_string(dir.file(FileTrustRecordStore::FILE_NAME)).expect("the file exists");

    assert_eq!(contents.lines().count(), 2, "header plus one record");
    assert_eq!(
        store.load_trust_record(alice()),
        Ok(Some(TrustRecord::rehydrate(
            alice(),
            VerificationState::Verified,
            true
        )))
    );
}

#[test]
fn saving_one_peer_leaves_the_others_alone() {
    let dir = TestDir::new("trust-independent");
    let store = store(&dir);

    store
        .save_trust_record(&TrustRecord::rehydrate(
            alice(),
            VerificationState::Verified,
            false,
        ))
        .expect("the save must land");
    store
        .save_trust_record(&TrustRecord::unverified(bob()))
        .expect("the save must land");

    assert_eq!(
        store.load_trust_record(alice()),
        Ok(Some(TrustRecord::rehydrate(
            alice(),
            VerificationState::Verified,
            false
        )))
    );
}

#[test]
fn lists_the_blocked_peers_and_nobody_else() {
    let dir = TestDir::new("trust-blocked-list");
    let store = store(&dir);

    store
        .save_trust_record(&TrustRecord::rehydrate(
            alice(),
            VerificationState::Unverified,
            true,
        ))
        .expect("the save must land");
    store
        .save_trust_record(&TrustRecord::rehydrate(
            bob(),
            VerificationState::Verified,
            false,
        ))
        .expect("the save must land");
    store
        .save_trust_record(&TrustRecord::rehydrate(
            carol(),
            VerificationState::Verified,
            true,
        ))
        .expect("the save must land");

    let mut blocked = store.list_blocked_peers().expect("the list must succeed");
    blocked.sort_unstable();
    let mut expected = vec![alice(), carol()];
    expected.sort_unstable();

    assert_eq!(blocked, expected);
}

#[test]
fn records_survive_a_restart() {
    let dir = TestDir::new("trust-restart");

    store(&dir)
        .save_trust_record(&TrustRecord::rehydrate(
            bob(),
            VerificationState::Verified,
            true,
        ))
        .expect("the save must land");

    // A fingerprint comparison performed once must not have to be repeated, and
    // a blocked peer must stay blocked.
    assert_eq!(
        store(&dir).load_trust_record(bob()),
        Ok(Some(TrustRecord::rehydrate(
            bob(),
            VerificationState::Verified,
            true
        )))
    );
}

#[test]
fn the_file_is_sorted_by_peer_id() {
    let dir = TestDir::new("trust-sorted");
    let store = store(&dir);

    for peer in [carol(), alice(), bob()] {
        store
            .save_trust_record(&TrustRecord::unverified(peer))
            .expect("the save must land");
    }

    let contents =
        fs::read_to_string(dir.file(FileTrustRecordStore::FILE_NAME)).expect("the file exists");
    let written: Vec<&str> = contents.lines().skip(1).collect();

    let mut peers = [alice(), bob(), carol()];
    peers.sort_unstable();
    let expected: Vec<String> = peers
        .iter()
        .map(|peer| {
            format!(
                "record {} unverified open",
                hex_bytes::encode(peer.as_bytes())
            )
        })
        .collect();

    assert_eq!(written, expected);
}

#[test]
fn a_malformed_line_is_corrupt_rather_than_a_panic() {
    let dir = TestDir::new("trust-malformed");
    let path = plant(
        &dir,
        "distro-trust-records 1\nrecord not-a-peer verified open\n",
    );

    assert_eq!(
        FileTrustRecordStore::at(&path).load_trust_record(alice()),
        Err(TrustRecordStoreError::Corrupt)
    );
}

#[test]
fn an_unknown_verification_word_is_corrupt() {
    let dir = TestDir::new("trust-unknown-word");
    let path = plant(
        &dir,
        &format!(
            "distro-trust-records 1\nrecord {} half-verified open\n",
            hex_bytes::encode(alice().as_bytes())
        ),
    );

    assert_eq!(
        FileTrustRecordStore::at(&path).list_blocked_peers(),
        Err(TrustRecordStoreError::Corrupt)
    );
}

#[test]
fn a_duplicate_peer_line_is_corrupt() {
    let dir = TestDir::new("trust-duplicate");
    let peer = hex_bytes::encode(alice().as_bytes());
    let path = plant(
        &dir,
        &format!(
            "distro-trust-records 1\nrecord {peer} verified open\nrecord {peer} unverified blocked\n"
        ),
    );

    // Quietly picking one of two answers about whether someone is blocked is
    // not a decision this store gets to make.
    assert_eq!(
        FileTrustRecordStore::at(&path).load_trust_record(alice()),
        Err(TrustRecordStoreError::Corrupt)
    );
}

#[test]
fn an_unknown_schema_version_is_reported_and_the_records_are_preserved() {
    let dir = TestDir::new("trust-future-version");
    let original = "distro-trust-records 7\nwhatever a later build writes\n";
    let path = plant(&dir, original);

    assert_eq!(
        FileTrustRecordStore::at(&path).load_trust_record(alice()),
        Err(TrustRecordStoreError::UnsupportedSchemaVersion { found: 7 })
    );
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_save_against_an_unknown_schema_version_refuses_rather_than_rewrites() {
    let dir = TestDir::new("trust-future-version-save");
    let original = "distro-trust-records 7\nwhatever a later build writes\n";
    let path = plant(&dir, original);

    assert_eq!(
        FileTrustRecordStore::at(&path).save_trust_record(&TrustRecord::unverified(alice())),
        Err(TrustRecordStoreError::UnsupportedSchemaVersion { found: 7 })
    );
    // The destructive rewrite S4 forbids is precisely this path: read fails,
    // so the write never happens.
    assert_eq!(
        fs::read_to_string(&path).expect("the file exists"),
        original
    );
}

#[test]
fn a_save_that_cannot_land_reports_write_failed() {
    let dir = TestDir::new("trust-write-fails");
    let store = FileTrustRecordStore::at(
        dir.file("never-created")
            .join(FileTrustRecordStore::FILE_NAME),
    );

    assert_eq!(
        store.save_trust_record(&TrustRecord::unverified(alice())),
        Err(TrustRecordStoreError::WriteFailed)
    );
}
