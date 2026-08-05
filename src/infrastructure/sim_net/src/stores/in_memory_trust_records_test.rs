use std::sync::Arc;

use identity::domain::TrustRecord;
use identity::ports::TrustRecordStorePort;
use messaging::ports::AuthorPolicyPort;
use shared_types::PeerId;

use crate::crypto::SimKeypair;
use crate::stores::{InMemoryTrustRecords, TrustRecordAuthorPolicy};

fn bob() -> PeerId {
    SimKeypair::derived(1, "bob").peer()
}

fn carol() -> PeerId {
    SimKeypair::derived(1, "carol").peer()
}

#[test]
fn an_unknown_peer_has_no_record_rather_than_an_error() {
    // Trust on first use: an unknown peer is the starting point, not a failure.
    let records = InMemoryTrustRecords::empty();

    assert_eq!(records.load_trust_record(bob()), Ok(None));
}

#[test]
fn saving_is_a_whole_record_upsert_keyed_by_peer() {
    let records = InMemoryTrustRecords::empty();
    let mut record = TrustRecord::unverified(bob());
    let _ = record.verify();

    records.save_trust_record(&record).expect("healthy store");
    records.save_trust_record(&record).expect("healthy store");

    assert_eq!(records.len(), 1);
    assert_eq!(records.load_trust_record(bob()), Ok(Some(record)));
}

#[test]
fn blocked_peers_are_listed_in_peer_id_order() {
    let records = InMemoryTrustRecords::empty();

    for peer in [bob(), carol()] {
        let mut record = TrustRecord::unverified(peer);
        record.block().expect("a fresh record is not blocked");
        records.save_trust_record(&record).expect("healthy store");
    }

    let mut expected = vec![bob(), carol()];
    expected.sort_unstable();

    assert_eq!(records.list_blocked_peers(), Ok(expected));
}

#[test]
fn verifying_a_peer_does_not_block_it() {
    let records = InMemoryTrustRecords::empty();
    let mut record = TrustRecord::unverified(bob());
    let _ = record.verify();
    records.save_trust_record(&record).expect("healthy store");

    assert_eq!(records.list_blocked_peers(), Ok(Vec::new()));
    assert!(!records.is_blocked(bob()));
}

#[test]
fn the_author_policy_reads_identitys_block_list() {
    // Invariant 11's cross-context wiring: messaging asks its own question and
    // identity holds the answer, with neither importing the other.
    let records = Arc::new(InMemoryTrustRecords::empty());
    let policy = TrustRecordAuthorPolicy::new(Arc::clone(&records));

    assert!(!policy.is_blocked(bob()));

    let mut record = TrustRecord::unverified(bob());
    record.block().expect("a fresh record is not blocked");
    records.save_trust_record(&record).expect("healthy store");

    assert!(policy.is_blocked(bob()));
    assert!(!policy.is_blocked(carol()));
}
