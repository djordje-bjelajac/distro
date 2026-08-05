use std::sync::Arc;

use identity::ports::{IdentityKeyStoreError, IdentityKeyStorePort};
use shared_types::PeerId;

use crate::crypto::SimKeypair;

/// Custody of one simulated peer's keypair (D5, AC9).
///
/// # What it models
///
/// `infra-store-fs` will keep the keypair in a file so a `PeerId` survives a
/// restart (OP-11). Here it is an `Arc<SimKeypair>` that the harness keeps
/// *outside* the peer's contexts and hands to each rebuild — so a restarted
/// peer loads the identity it had, exactly as AC9 requires, while everything
/// the process held in memory is discarded.
///
/// The port is load-or-create and idempotent; the create half already happened
/// when the harness minted the peer, so this only ever loads. Secret bytes
/// never cross it: what comes back is the public [`PeerId`], and signing is a
/// separate port (`SimSigner`).
pub struct SimKeyStore {
    keypair: Arc<SimKeypair>,
}

impl SimKeyStore {
    /// A store holding `keypair`.
    pub const fn new(keypair: Arc<SimKeypair>) -> Self {
        Self { keypair }
    }
}

impl IdentityKeyStorePort for SimKeyStore {
    fn load_or_create_local_peer(&self) -> Result<PeerId, IdentityKeyStoreError> {
        Ok(self.keypair.peer())
    }
}
