use std::sync::Arc;

use shared_types::PeerId;

use crate::application::LocalIdentityState;
use crate::application::queries::{
    GetLocalIdentity, GetLocalIdentityHandler, GetPeerTrustState, GetPeerTrustStateHandler,
    ListBlockedPeers, ListBlockedPeersHandler,
};
use crate::ports::{
    IdentityQueryPort, LocalIdentitySummary, PeerTrustState, TrustRecordStoreError,
    TrustRecordStorePort,
};

/// The read half of this context's inbound surface: one
/// [`IdentityQueryPort`] implementation over the three query handlers.
///
/// Every method reads and returns; none writes, and none can, because no
/// handler behind it calls a store's write method. Wired over the same
/// [`LocalIdentityState`] as
/// [`IdentityCommandService`](crate::application::commands::IdentityCommandService)
/// so a rename issued as a command is immediately visible as a query.
#[derive(Clone)]
pub struct IdentityQueryService {
    local_identity: GetLocalIdentityHandler,
    peer_trust_state: GetPeerTrustStateHandler,
    blocked_peers: ListBlockedPeersHandler,
}

impl IdentityQueryService {
    /// Wires the query side over the shared [`LocalIdentityState`] and the
    /// trust record store.
    ///
    /// It takes no key store: nothing on the read path ever needs the
    /// keypair — the `PeerId` it reports comes from the identity the command
    /// side already assumed.
    pub fn new(
        state: Arc<LocalIdentityState>,
        trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
    ) -> Self {
        Self {
            local_identity: GetLocalIdentityHandler::new(state),
            peer_trust_state: GetPeerTrustStateHandler::new(Arc::clone(&trust_records)),
            blocked_peers: ListBlockedPeersHandler::new(trust_records),
        }
    }
}

impl IdentityQueryPort for IdentityQueryService {
    fn local_identity(&self) -> Option<LocalIdentitySummary> {
        self.local_identity.handle(GetLocalIdentity)
    }

    fn peer_trust_state(&self, peer: PeerId) -> Result<PeerTrustState, TrustRecordStoreError> {
        self.peer_trust_state.handle(GetPeerTrustState { peer })
    }

    fn blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        self.blocked_peers.handle(ListBlockedPeers)
    }
}
