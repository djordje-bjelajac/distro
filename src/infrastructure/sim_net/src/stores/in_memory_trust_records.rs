use std::collections::BTreeMap;
use std::sync::Mutex;

use identity::domain::TrustRecord;
use identity::ports::{TrustRecordStoreError, TrustRecordStorePort};
use shared_types::PeerId;

use crate::stores::guard;

/// This peer's verification and block state, in memory (canvas §2.1).
///
/// # It outlives the process
///
/// Both halves of trust are meant to survive a restart: a fingerprint
/// comparison a user performed once must not have to be repeated, and a blocked
/// peer must stay blocked. The harness therefore keeps this store outside a
/// peer's contexts and hands the same instance to every rebuild.
///
/// # The one place invariant 11 crosses a context boundary
///
/// The block list is `identity`'s. `messaging` asks its own `AuthorPolicyPort`
/// whether an author is refused, and the composition root joins the two —
/// neither context importing the other, no port trait in `shared_types`
/// (canvas §2.4, §4). In this crate that join is
/// [`TrustRecordAuthorPolicy`](crate::stores::TrustRecordAuthorPolicy), which
/// reads *this* store. Blocking a peer through `identity`'s command port
/// therefore stops its envelopes at `messaging`'s boundary, with no wiring a
/// scenario has to remember.
#[derive(Debug, Default)]
pub struct InMemoryTrustRecords {
    records: Mutex<BTreeMap<PeerId, TrustRecord>>,
}

impl InMemoryTrustRecords {
    /// An empty store: every peer at the trust-on-first-use starting point.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether `peer` is blocked, without going through the port.
    ///
    /// The question `AuthorPolicyPort` asks, answered directly so the policy
    /// adapter stays a two-line delegation.
    pub fn is_blocked(&self, peer: PeerId) -> bool {
        guard(&self.records)
            .get(&peer)
            .is_some_and(TrustRecord::is_blocked)
    }

    /// How many records the store holds.
    pub fn len(&self) -> usize {
        guard(&self.records).len()
    }

    /// Whether nothing has been verified or blocked.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TrustRecordStorePort for InMemoryTrustRecords {
    fn load_trust_record(
        &self,
        peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        Ok(guard(&self.records).get(&peer).cloned())
    }

    fn save_trust_record(&self, record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        guard(&self.records).insert(record.peer(), record.clone());
        Ok(())
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        Ok(guard(&self.records)
            .values()
            .filter(|record| record.is_blocked())
            .map(TrustRecord::peer)
            .collect())
    }
}
