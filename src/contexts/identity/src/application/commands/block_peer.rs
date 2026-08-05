use std::sync::Arc;

use shared_types::PeerId;

use crate::domain::TrustRecord;
use crate::domain::events::PeerBlocked;
use crate::ports::{PeerTrustCommandError, TrustRecordStorePort};

/// Stop accepting this peer's traffic (invariant 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPeer {
    /// The peer to block.
    pub peer: PeerId,
}

/// Handles [`BlockPeer`]: sets the local, unannounced block flag.
///
/// A peer with no stored record can be blocked — blocking someone you have
/// never verified is the common case — so the handler starts from
/// [`TrustRecord::unverified`]. Blocking leaves verification untouched: the
/// record answers two independent questions, and unblocking later restores the
/// peer to exactly the verification it always had.
///
/// Blocking an already blocked peer is rejected rather than silently accepted
/// ([`PeerTrustCommandError::Rejected`]): the caller's view of this peer is
/// stale, and swallowing it would let a UI report a change that never
/// happened. A store failure is reported as
/// [`PeerTrustCommandError::Store`] and kept distinct — "this would change
/// nothing" and "this change may not have survived" call for different
/// responses, and only the second means the block is not in force.
///
/// The block is purely local: nothing is published, and the network learns
/// nothing. The composition root reads the resulting list through
/// `IdentityQueryPort` and hands it to `messaging`'s own `AuthorPolicyPort`.
#[derive(Clone)]
pub struct BlockPeerHandler {
    trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
}

impl BlockPeerHandler {
    pub fn new(trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self { trust_records }
    }

    pub fn handle(&self, command: BlockPeer) -> Result<PeerBlocked, PeerTrustCommandError> {
        let mut record = self
            .trust_records
            .load_trust_record(command.peer)?
            .unwrap_or_else(|| TrustRecord::unverified(command.peer));

        let event = record.block()?;

        self.trust_records.save_trust_record(&record)?;
        Ok(event)
    }
}
