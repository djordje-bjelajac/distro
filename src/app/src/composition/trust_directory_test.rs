use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use identity::domain::{TrustRecord, VerificationState};
use identity::ports::{TrustRecordStoreError, TrustRecordStorePort};
use messaging::ports::AuthorPolicyPort;
use shared_types::PeerId;

use crate::composition::TrustDirectory;
use crate::test_peers::{alice, bob, carol};

/// An in-memory stand-in for `FileTrustRecordStore` that can be made to fail.
#[derive(Default)]
struct FakeTrustRecords {
    records: Mutex<BTreeMap<PeerId, TrustRecord>>,
    fails: Mutex<bool>,
}

impl FakeTrustRecords {
    fn save(&self, record: TrustRecord) {
        self.records
            .lock()
            .expect("no panic")
            .insert(record.peer(), record);
    }

    fn block(&self, peer: PeerId) {
        let mut record = TrustRecord::unverified(peer);
        record.block().expect("a fresh record is not blocked");
        self.save(record);
    }

    fn verify(&self, peer: PeerId) {
        let mut record = TrustRecord::unverified(peer);
        record.verify();
        self.save(record);
    }

    fn start_failing(&self) {
        *self.fails.lock().expect("no panic") = true;
    }

    fn guard(&self) -> Result<(), TrustRecordStoreError> {
        if *self.fails.lock().expect("no panic") {
            return Err(TrustRecordStoreError::Unreadable);
        }
        Ok(())
    }
}

impl TrustRecordStorePort for FakeTrustRecords {
    fn load_trust_record(
        &self,
        peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        self.guard()?;
        Ok(self.records.lock().expect("no panic").get(&peer).cloned())
    }

    fn save_trust_record(&self, record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        self.guard()?;
        self.save(record.clone());
        Ok(())
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        self.guard()?;
        Ok(self
            .records
            .lock()
            .expect("no panic")
            .values()
            .filter(|record| record.is_blocked())
            .map(TrustRecord::peer)
            .collect())
    }
}

fn directory() -> (Arc<FakeTrustRecords>, TrustDirectory) {
    let records = Arc::new(FakeTrustRecords::default());
    let directory = TrustDirectory::new(Arc::clone(&records) as Arc<_>);
    (records, directory)
}

#[test]
fn before_any_refresh_nobody_is_blocked() {
    // A fresh install's starting point, and the only safe state to begin in.
    let (_records, directory) = directory();

    assert!(!directory.is_blocked(alice()));
    assert_eq!(directory.trust_of(alice()), Default::default());
}

#[test]
fn a_blocked_peer_is_refused_after_a_refresh() {
    // Invariant 11 through the seam: `identity` holds the list, `messaging`
    // asks its own question, the root joins them.
    let (records, directory) = directory();
    records.block(alice());

    directory.refresh(&[]).expect("the fake store reads");

    assert!(directory.is_blocked(alice()));
    assert!(!directory.is_blocked(bob()));
}

#[test]
fn a_blocked_peer_stays_blocked_when_it_is_not_in_the_roster() {
    // A peer that went offline is still blocked; a snapshot built only from
    // the roster would unblock it the moment it left.
    let (records, directory) = directory();
    records.block(carol());

    directory
        .refresh(&[alice(), bob()])
        .expect("the fake store reads");

    assert!(directory.is_blocked(carol()));
}

#[test]
fn verification_and_blocking_are_reported_separately() {
    // The two axes are orthogonal in `TrustRecord`, and a UI must be able to
    // render a peer that is both.
    let (records, directory) = directory();
    let mut record = TrustRecord::unverified(alice());
    record.verify();
    record
        .block()
        .expect("a verified record can still be blocked");
    records.save(record);

    directory.refresh(&[alice()]).expect("the fake store reads");

    let trust = directory.trust_of(alice());
    assert_eq!(trust.verification, VerificationState::Verified);
    assert!(trust.blocked);
    assert!(trust.is_verified());
}

#[test]
fn a_verified_peer_is_not_blocked() {
    let (records, directory) = directory();
    records.verify(bob());

    directory.refresh(&[bob()]).expect("the fake store reads");

    assert!(directory.trust_of(bob()).is_verified());
    assert!(!directory.is_blocked(bob()));
}

#[test]
fn unblocking_takes_effect_on_the_next_refresh() {
    let (records, directory) = directory();
    records.block(alice());
    directory.refresh(&[]).expect("the fake store reads");
    assert!(directory.is_blocked(alice()));

    records.save(TrustRecord::unverified(alice()));
    directory.refresh(&[alice()]).expect("the fake store reads");

    assert!(!directory.is_blocked(alice()));
}

#[test]
fn a_failed_refresh_keeps_the_last_known_decision() {
    // Discarding the snapshot because a read failed would silently unblock
    // everyone — the one outcome the port says has no safe default.
    let (records, directory) = directory();
    records.block(alice());
    directory.refresh(&[]).expect("the fake store reads");

    records.start_failing();
    let refreshed = directory.refresh(&[alice()]);

    assert_eq!(refreshed, Err(TrustRecordStoreError::Unreadable));
    assert!(directory.is_blocked(alice()));
}

#[test]
fn the_blocked_list_is_reported_in_peer_order() {
    let (records, directory) = directory();
    records.block(carol());
    records.block(alice());
    records.block(bob());

    directory.refresh(&[]).expect("the fake store reads");

    let mut expected = vec![alice(), bob(), carol()];
    expected.sort();
    assert_eq!(directory.blocked_peers(), expected);
}
