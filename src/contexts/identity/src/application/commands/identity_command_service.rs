use std::sync::Arc;

use shared_types::PeerId;

use crate::application::LocalIdentityState;
use crate::application::commands::{
    BlockPeer, BlockPeerHandler, InitializeLocalIdentity, InitializeLocalIdentityHandler,
    SetDisplayName, SetDisplayNameHandler, UnblockPeer, UnblockPeerHandler, VerifyPeer,
    VerifyPeerHandler,
};
use crate::domain::DisplayName;
use crate::domain::events::{DisplayNameChanged, PeerBlocked, PeerUnblocked, PeerVerified};
use crate::ports::{
    IdentityCommandPort, IdentityKeyStoreError, IdentityKeyStorePort, LocalIdentityAssumption,
    PeerTrustCommandError, SetDisplayNameError, TrustRecordStoreError, TrustRecordStorePort,
};

/// The write half of this context's inbound surface: one
/// [`IdentityCommandPort`] implementation over the five command handlers.
///
/// It holds handlers rather than reimplementing them so each use case keeps
/// its own file, its own tests, and its own dependencies — this type adds
/// only the translation from the port's domain-typed arguments to the
/// imperative command DTOs, and contains no decision of its own. Nothing here
/// reads for display; that is
/// [`IdentityQueryService`](crate::application::queries::IdentityQueryService).
#[derive(Clone)]
pub struct IdentityCommandService {
    initialize_local_identity: InitializeLocalIdentityHandler,
    set_display_name: SetDisplayNameHandler,
    verify_peer: VerifyPeerHandler,
    block_peer: BlockPeerHandler,
    unblock_peer: UnblockPeerHandler,
}

impl IdentityCommandService {
    /// Wires the command side over one shared [`LocalIdentityState`] and the
    /// context's two outbound stores.
    pub fn new(
        state: Arc<LocalIdentityState>,
        key_store: Arc<dyn IdentityKeyStorePort + Send + Sync>,
        trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
    ) -> Self {
        Self {
            initialize_local_identity: InitializeLocalIdentityHandler::new(
                Arc::clone(&state),
                key_store,
            ),
            set_display_name: SetDisplayNameHandler::new(state),
            verify_peer: VerifyPeerHandler::new(Arc::clone(&trust_records)),
            block_peer: BlockPeerHandler::new(Arc::clone(&trust_records)),
            unblock_peer: UnblockPeerHandler::new(trust_records),
        }
    }
}

impl IdentityCommandPort for IdentityCommandService {
    fn initialize_local_identity(
        &self,
        display_name: Option<DisplayName>,
    ) -> Result<LocalIdentityAssumption, IdentityKeyStoreError> {
        self.initialize_local_identity
            .handle(InitializeLocalIdentity { display_name })
    }

    fn set_display_name(
        &self,
        requested: &str,
    ) -> Result<Option<DisplayNameChanged>, SetDisplayNameError> {
        self.set_display_name.handle(SetDisplayName::new(requested))
    }

    fn verify_peer(&self, peer: PeerId) -> Result<Option<PeerVerified>, TrustRecordStoreError> {
        self.verify_peer.handle(VerifyPeer { peer })
    }

    fn block_peer(&self, peer: PeerId) -> Result<PeerBlocked, PeerTrustCommandError> {
        self.block_peer.handle(BlockPeer { peer })
    }

    fn unblock_peer(&self, peer: PeerId) -> Result<PeerUnblocked, PeerTrustCommandError> {
        self.unblock_peer.handle(UnblockPeer { peer })
    }
}
