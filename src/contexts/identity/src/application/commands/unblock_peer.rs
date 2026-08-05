use std::sync::Arc;

use shared_types::PeerId;

use crate::domain::TrustRecordError;
use crate::domain::events::PeerUnblocked;
use crate::ports::{PeerTrustCommandError, TrustRecordStorePort};

/// Start accepting this peer's traffic again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnblockPeer {
    /// The peer to unblock.
    pub peer: PeerId,
}

/// Handles [`UnblockPeer`]: clears the local block flag.
///
/// Unlike [`BlockPeer`](crate::application::commands::BlockPeer) and
/// [`VerifyPeer`](crate::application::commands::VerifyPeer), a peer with no
/// stored record is **not** materialised here: an unknown peer is not blocked,
/// so the command would change nothing and is rejected with
/// [`TrustRecordError::NotBlocked`] — the same answer the domain gives for a
/// known-but-unblocked peer, and the answer that keeps a stray unblock from
/// littering the store with default records.
///
/// Unblocking restores nothing else: verification was never touched by the
/// flag, so the peer returns to exactly the state it kept throughout.
#[derive(Clone)]
pub struct UnblockPeerHandler {
    trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
}

impl UnblockPeerHandler {
    pub fn new(trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self { trust_records }
    }

    pub fn handle(&self, command: UnblockPeer) -> Result<PeerUnblocked, PeerTrustCommandError> {
        let Some(mut record) = self.trust_records.load_trust_record(command.peer)? else {
            return Err(PeerTrustCommandError::Rejected(
                TrustRecordError::NotBlocked,
            ));
        };

        let event = record.unblock()?;

        self.trust_records.save_trust_record(&record)?;
        Ok(event)
    }
}
