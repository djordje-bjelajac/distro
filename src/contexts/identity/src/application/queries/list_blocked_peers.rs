use std::sync::Arc;

use shared_types::PeerId;

use crate::ports::{TrustRecordStoreError, TrustRecordStorePort};

/// Ask for every peer the local user is currently blocking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListBlockedPeers;

/// Handles [`ListBlockedPeers`]: the local block list, in a stable order.
///
/// The store's order is implementation-defined, so this handler sorts by
/// `PeerId` before returning. That is a determinism requirement (S5), not
/// cosmetics: the list is what the composition root feeds to `messaging`'s
/// `AuthorPolicyPort` (invariant 11), and a list whose order depended on
/// insertion history would make a diagnostic trace differ between two runs of
/// the same scenario.
///
/// Sorting is by key bytes rather than by display name deliberately — a name
/// takes no part in identity or lookup (invariant 8), and two peers may share
/// one.
#[derive(Clone)]
pub struct ListBlockedPeersHandler {
    trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
}

impl ListBlockedPeersHandler {
    pub fn new(trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self { trust_records }
    }

    pub fn handle(&self, _query: ListBlockedPeers) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        let mut blocked = self.trust_records.list_blocked_peers()?;
        blocked.sort_unstable();
        Ok(blocked)
    }
}
