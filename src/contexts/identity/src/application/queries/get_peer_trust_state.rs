use std::sync::Arc;

use shared_types::{Fingerprint, PeerId};

use crate::domain::TrustRecord;
use crate::ports::{PeerTrustState, TrustRecordStoreError, TrustRecordStorePort};

/// Ask what this peer locally believes about one remote peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetPeerTrustState {
    /// The peer being asked about.
    pub peer: PeerId,
}

/// Handles [`GetPeerTrustState`]: reports verification and blocking for one
/// peer, plus the fingerprint to compare before verifying (AC6).
///
/// A peer with no stored record reports the trust-on-first-use starting point
/// (`Unverified`, not blocked) and **stores nothing**. That distinction is the
/// point of keeping this on the query side: a roster redraw asks about every
/// peer it can see, and a query that materialised a default record would turn
/// rendering into a write amplifier and make "known peers" mean "peers we once
/// drew".
#[derive(Clone)]
pub struct GetPeerTrustStateHandler {
    trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
}

impl GetPeerTrustStateHandler {
    pub fn new(trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self { trust_records }
    }

    pub fn handle(
        &self,
        query: GetPeerTrustState,
    ) -> Result<PeerTrustState, TrustRecordStoreError> {
        let record = self
            .trust_records
            .load_trust_record(query.peer)?
            .unwrap_or_else(|| TrustRecord::unverified(query.peer));

        Ok(PeerTrustState {
            peer: record.peer(),
            verification: record.verification(),
            blocked: record.is_blocked(),
            fingerprint: Fingerprint::of(&record.peer()),
        })
    }
}
