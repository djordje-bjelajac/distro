use std::sync::Arc;

use shared_types::PeerId;

use crate::domain::TrustRecord;
use crate::domain::events::PeerVerified;
use crate::ports::{TrustRecordStoreError, TrustRecordStorePort};

/// Record that the local user compared this peer's fingerprint out-of-band and
/// it matched (D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyPeer {
    /// The peer whose key was confirmed.
    pub peer: PeerId,
}

/// Handles [`VerifyPeer`]: moves a peer up the trust-on-first-use ladder.
///
/// A peer that has no stored record yet is the ordinary case — the user is
/// verifying someone they just met — so the handler starts from
/// [`TrustRecord::unverified`] rather than failing. Re-verifying an already
/// verified peer succeeds with `Ok(None)` and performs **no write**: the
/// domain reports no transition, and writing an unchanged record would put
/// avoidable I/O (and an avoidable failure mode) on a path where nothing
/// happened.
///
/// Verification never touches the blocked flag; the two axes are orthogonal,
/// so a user may confirm the key of a peer whose traffic they are dropping.
#[derive(Clone)]
pub struct VerifyPeerHandler {
    trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
}

impl VerifyPeerHandler {
    pub fn new(trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self { trust_records }
    }

    pub fn handle(
        &self,
        command: VerifyPeer,
    ) -> Result<Option<PeerVerified>, TrustRecordStoreError> {
        let mut record = self
            .trust_records
            .load_trust_record(command.peer)?
            .unwrap_or_else(|| TrustRecord::unverified(command.peer));

        let Some(event) = record.verify() else {
            return Ok(None);
        };

        self.trust_records.save_trust_record(&record)?;
        Ok(Some(event))
    }
}
