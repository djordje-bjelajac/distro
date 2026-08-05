use std::sync::Arc;

use crate::application::commands::{VerifyPeer, VerifyPeerHandler};
use crate::domain::events::PeerVerified;
use crate::domain::{TrustRecord, VerificationState};
use crate::ports::port_fakes::{
    InMemoryTrustRecordStore, UnusableTrustRecordStore, UnwritableTrustRecordStore,
};
use crate::ports::{TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

fn handler_over(store: &Arc<InMemoryTrustRecordStore>) -> VerifyPeerHandler {
    VerifyPeerHandler::new(Arc::clone(store) as Arc<dyn TrustRecordStorePort + Send + Sync>)
}

#[test]
fn verifying_a_peer_seen_for_the_first_time_stores_a_verified_record() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());

    let event = handler_over(&store)
        .handle(VerifyPeer {
            peer: test_peers::bob(),
        })
        .expect("verifying an unknown peer is the trust-on-first-use case");

    assert_eq!(
        event,
        Some(PeerVerified {
            peer: test_peers::bob()
        })
    );
    assert!(
        store
            .stored(test_peers::bob())
            .expect("the record was written")
            .is_verified()
    );
    assert_eq!(store.saves(), 1);
}

#[test]
fn verifying_again_emits_nothing_and_writes_nothing() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());
    let handler = handler_over(&store);
    let command = VerifyPeer {
        peer: test_peers::bob(),
    };

    handler.handle(command).expect("first verification");
    let second = handler.handle(command).expect("re-verifying is idempotent");

    assert_eq!(
        second, None,
        "the post-condition already held, so nothing is announced"
    );
    assert_eq!(store.saves(), 1, "an unchanged record is not rewritten");
}

#[test]
fn verification_leaves_the_blocked_flag_untouched() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::rehydrate(test_peers::bob(), VerificationState::Unverified, true),
    ]));

    handler_over(&store)
        .handle(VerifyPeer {
            peer: test_peers::bob(),
        })
        .expect("a blocked peer's key can still be confirmed out-of-band");

    let record = store.stored(test_peers::bob()).expect("record");
    assert!(record.is_verified());
    assert!(record.is_blocked(), "the two axes are orthogonal");
}

#[test]
fn a_store_read_failure_surfaces_as_a_typed_error() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> =
        Arc::new(UnusableTrustRecordStore(TrustRecordStoreError::Unreadable));

    let outcome = VerifyPeerHandler::new(store).handle(VerifyPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(outcome, Err(TrustRecordStoreError::Unreadable));
}

#[test]
fn a_store_write_failure_is_reported_rather_than_claimed_as_success() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> = Arc::new(UnwritableTrustRecordStore(
        TrustRecordStoreError::WriteFailed,
    ));

    let outcome = VerifyPeerHandler::new(store).handle(VerifyPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(TrustRecordStoreError::WriteFailed),
        "a verification the user would have to repeat must not read as done"
    );
}
