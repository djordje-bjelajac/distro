use std::sync::Arc;

use crate::application::commands::{BlockPeer, BlockPeerHandler};
use crate::domain::events::PeerBlocked;
use crate::domain::{TrustRecord, TrustRecordError, VerificationState};
use crate::ports::port_fakes::{
    InMemoryTrustRecordStore, UnusableTrustRecordStore, UnwritableTrustRecordStore,
};
use crate::ports::{PeerTrustCommandError, TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

fn handler_over(store: &Arc<InMemoryTrustRecordStore>) -> BlockPeerHandler {
    BlockPeerHandler::new(Arc::clone(store) as Arc<dyn TrustRecordStorePort + Send + Sync>)
}

#[test]
fn blocking_a_peer_seen_for_the_first_time_stores_a_blocked_record() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());

    let event = handler_over(&store)
        .handle(BlockPeer {
            peer: test_peers::bob(),
        })
        .expect("a peer can be blocked before it is ever verified");

    assert_eq!(
        event,
        PeerBlocked {
            peer: test_peers::bob()
        }
    );
    assert!(
        store
            .stored(test_peers::bob())
            .expect("record")
            .is_blocked()
    );
    assert_eq!(
        store.list_blocked_peers().expect("list"),
        vec![test_peers::bob()]
    );
}

#[test]
fn blocking_a_verified_peer_preserves_the_verification() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::rehydrate(test_peers::bob(), VerificationState::Verified, false),
    ]));

    handler_over(&store)
        .handle(BlockPeer {
            peer: test_peers::bob(),
        })
        .expect("blocking a verified peer is legitimate");

    let record = store.stored(test_peers::bob()).expect("record");
    assert!(record.is_blocked());
    assert!(
        record.is_verified(),
        "blocking answers a different question than verifying"
    );
}

#[test]
fn blocking_an_already_blocked_peer_is_rejected_and_writes_nothing() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());
    let handler = handler_over(&store);
    let command = BlockPeer {
        peer: test_peers::bob(),
    };

    handler.handle(command).expect("first block");
    let second = handler.handle(command);

    assert_eq!(
        second,
        Err(PeerTrustCommandError::Rejected(
            TrustRecordError::AlreadyBlocked
        )),
        "a command that would change nothing surfaces the caller's stale view"
    );
    assert_eq!(store.saves(), 1, "the rejected command wrote nothing");
}

#[test]
fn a_store_read_failure_is_reported_as_a_store_failure_not_a_domain_rejection() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> =
        Arc::new(UnusableTrustRecordStore(TrustRecordStoreError::Unreadable));

    let outcome = BlockPeerHandler::new(store).handle(BlockPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(PeerTrustCommandError::Store(
            TrustRecordStoreError::Unreadable
        ))
    );
}

#[test]
fn a_store_write_failure_is_reported_rather_than_claimed_as_a_block() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> = Arc::new(UnwritableTrustRecordStore(
        TrustRecordStoreError::WriteFailed,
    ));

    let outcome = BlockPeerHandler::new(store).handle(BlockPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(PeerTrustCommandError::Store(
            TrustRecordStoreError::WriteFailed
        )),
        "a UI must never show \"blocked\" for a write that did not land"
    );
}
