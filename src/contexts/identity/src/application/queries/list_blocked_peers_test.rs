use std::sync::Arc;

use crate::application::queries::{ListBlockedPeers, ListBlockedPeersHandler};
use crate::domain::{TrustRecord, VerificationState};
use crate::ports::port_fakes::{InMemoryTrustRecordStore, UnusableTrustRecordStore};
use crate::ports::{TrustRecordStoreError, TrustRecordStorePort};
use crate::test_peers;

fn handler_over(store: &Arc<InMemoryTrustRecordStore>) -> ListBlockedPeersHandler {
    ListBlockedPeersHandler::new(Arc::clone(store) as Arc<dyn TrustRecordStorePort + Send + Sync>)
}

fn blocked(peer: shared_types::PeerId) -> TrustRecord {
    TrustRecord::rehydrate(peer, VerificationState::Unverified, true)
}

#[test]
fn lists_exactly_the_peers_whose_records_are_blocked() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([
        TrustRecord::unverified(test_peers::alice()),
        blocked(test_peers::bob()),
        TrustRecord::rehydrate(test_peers::carol(), VerificationState::Verified, true),
    ]));

    let listed = handler_over(&store)
        .handle(ListBlockedPeers)
        .expect("listing");

    assert_eq!(
        listed,
        vec![test_peers::bob(), test_peers::carol()],
        "a verified peer that is blocked still belongs on the list"
    );
}

#[test]
fn an_empty_store_yields_an_empty_list_rather_than_an_error() {
    let store = Arc::new(InMemoryTrustRecordStore::empty());

    assert_eq!(
        handler_over(&store)
            .handle(ListBlockedPeers)
            .expect("listing"),
        Vec::new()
    );
}

#[test]
fn the_order_is_stable_regardless_of_how_the_store_returns_it() {
    let ascending = Arc::new(InMemoryTrustRecordStore::seeded_with([
        blocked(test_peers::bob()),
        blocked(test_peers::carol()),
    ]));
    let descending = Arc::new(InMemoryTrustRecordStore::seeded_with([
        blocked(test_peers::carol()),
        blocked(test_peers::bob()),
    ]));

    let from_ascending = handler_over(&ascending)
        .handle(ListBlockedPeers)
        .expect("listing");
    let from_descending = handler_over(&descending)
        .handle(ListBlockedPeers)
        .expect("listing");

    assert_eq!(
        from_ascending, from_descending,
        "the block list is a determinism-sensitive read (S5): insertion order must not leak"
    );
    let mut sorted = from_ascending.clone();
    sorted.sort_unstable();
    assert_eq!(from_ascending, sorted);
}

#[test]
fn listing_never_writes() {
    let store = Arc::new(InMemoryTrustRecordStore::seeded_with([blocked(
        test_peers::bob(),
    )]));
    let handler = handler_over(&store);

    for _ in 0..3 {
        handler.handle(ListBlockedPeers).expect("listing");
    }

    assert_eq!(store.saves(), 0);
}

#[test]
fn a_store_failure_surfaces_as_a_typed_error() {
    let store: Arc<dyn TrustRecordStorePort + Send + Sync> =
        Arc::new(UnusableTrustRecordStore(TrustRecordStoreError::Unreadable));

    assert_eq!(
        ListBlockedPeersHandler::new(store).handle(ListBlockedPeers),
        Err(TrustRecordStoreError::Unreadable)
    );
}
