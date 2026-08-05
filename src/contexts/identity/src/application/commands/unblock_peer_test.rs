use std::sync::Arc;

use crate::application::commands::{UnblockPeer, UnblockPeerHandler};
use crate::domain::events::PeerUnblocked;
use crate::domain::{TrustRecord, TrustRecordError, VerificationState};
use crate::ports::port_fakes::{InMemoryTrustRecordStore, UnusableTrustRecordStore};
use crate::ports::{PeerTrustCommandError, TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

fn handler_over(store: &Arc<InMemoryTrustRecordStore>) -> UnblockPeerHandler {
    UnblockPeerHandler::new(Arc::clone(store) as Arc<dyn TrustRecordStorePort + Send + Sync>)
}

#[test]
fn unblocking_clears_the_flag_and_restores_nothing_else() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::rehydrate(test_peers::bob(), VerificationState::Verified, true),
    ]));

    let event = handler_over(&store)
        .handle(UnblockPeer {
            peer: test_peers::bob(),
        })
        .expect("a blocked peer can be unblocked");

    assert_eq!(
        event,
        PeerUnblocked {
            peer: test_peers::bob()
        }
    );
    let record = store.stored(test_peers::bob()).expect("record");
    assert!(!record.is_blocked());
    assert!(
        record.is_verified(),
        "the verification was never touched by the flag"
    );
    assert_eq!(store.list_blocked_peers().expect("list"), Vec::new());
}

#[test]
fn unblocking_a_peer_that_is_not_blocked_is_rejected_and_writes_nothing() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::unverified(test_peers::bob()),
    ]));

    let outcome = handler_over(&store).handle(UnblockPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(PeerTrustCommandError::Rejected(
            TrustRecordError::NotBlocked
        ))
    );
    assert_eq!(store.saves(), 0);
}

#[test]
fn unblocking_a_peer_that_was_never_seen_creates_no_record() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());

    let outcome = handler_over(&store).handle(UnblockPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(PeerTrustCommandError::Rejected(
            TrustRecordError::NotBlocked
        )),
        "an unknown peer is not blocked, so unblocking it changes nothing"
    );
    assert_eq!(store.saves(), 0);
    assert_eq!(store.stored(test_peers::bob()), None);
}

#[test]
fn a_store_failure_is_reported_as_a_store_failure() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> =
        Arc::new(UnusableTrustRecordStore(TrustRecordStoreError::Corrupt));

    let outcome = UnblockPeerHandler::new(store).handle(UnblockPeer {
        peer: test_peers::bob(),
    });

    assert_eq!(
        outcome,
        Err(PeerTrustCommandError::Store(TrustRecordStoreError::Corrupt))
    );
}
