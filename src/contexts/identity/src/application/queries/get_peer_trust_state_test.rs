use std::sync::Arc;

use shared_types::Fingerprint;

use crate::application::queries::{GetPeerTrustState, GetPeerTrustStateHandler};
use crate::domain::{TrustRecord, VerificationState};
use crate::ports::port_fakes::{InMemoryTrustRecordStore, UnusableTrustRecordStore};
use crate::ports::{PeerTrustState, TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

fn handler_over(store: &Arc<InMemoryTrustRecordStore>) -> GetPeerTrustStateHandler {
    GetPeerTrustStateHandler::new(Arc::clone(store) as Arc<dyn TrustRecordStorePort + Send + Sync>)
}

#[test]
fn reports_the_stored_record_on_both_axes() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::rehydrate(test_peers::bob(), VerificationState::Verified, true),
    ]));

    let state = handler_over(&store)
        .handle(GetPeerTrustState {
            peer: test_peers::bob(),
        })
        .expect("reading a known peer");

    assert_eq!(
        state,
        PeerTrustState {
            peer: test_peers::bob(),
            verification: VerificationState::Verified,
            blocked: true,
            fingerprint: Fingerprint::of(&test_peers::bob()),
        }
    );
}

#[test]
fn an_unknown_peer_reads_as_the_trust_on_first_use_default_and_stores_nothing() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());

    let state = handler_over(&store)
        .handle(GetPeerTrustState {
            peer: test_peers::carol(),
        })
        .expect("an unseen peer is not an error");

    assert_eq!(state.verification, VerificationState::Unverified);
    assert!(!state.blocked);
    assert_eq!(store.saves(), 0, "a query never writes");
    assert_eq!(
        store.stored(test_peers::carol()),
        None,
        "asking about a peer must not create a record for it"
    );
}

#[test]
fn repeated_reads_never_write() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::unverified(test_peers::bob()),
    ]));
    let handler = handler_over(&store);
    let query = GetPeerTrustState {
        peer: test_peers::bob(),
    };

    for _ in 0..3 {
        handler.handle(query).expect("read");
    }

    assert_eq!(store.saves(), 0);
    assert_eq!(
        store.loads(),
        3,
        "each read went through the port exactly once"
    );
}

#[test]
fn a_store_failure_surfaces_as_a_typed_error() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> =
        Arc::new(UnusableTrustRecordStore(TrustRecordStoreError::Corrupt));

    let outcome = GetPeerTrustStateHandler::new(store).handle(GetPeerTrustState {
        peer: test_peers::bob(),
    });

    assert_eq!(outcome, Err(TrustRecordStoreError::Corrupt));
}
