use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::ports::LocalIdentitySummary;

/// Ask who this process is on the network.
///
/// A unit query: the local identity is singular, so there is nothing to select
/// by. It exists as a type anyway so the query side reads like the command
/// side and a future filter has somewhere to live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetLocalIdentity;

/// Handles [`GetLocalIdentity`]: reports the local peer, its display name, and
/// the fingerprint a user reads aloud for out-of-band verification (AC6).
///
/// Returns `None` before `InitializeLocalIdentity` has run rather than
/// bootstrapping on demand: a query must not create state, and a UI that can
/// draw before the keystore has answered needs to be able to say "starting".
///
/// The fingerprint is derived on each read from the `PeerId` — it is a pure
/// function of the key, so there is nothing to cache and no way for it to
/// drift from the identity it describes.
#[derive(Clone)]
pub struct GetLocalIdentityHandler {
    state: Arc<LocalIdentityState>,
}

impl GetLocalIdentityHandler {
    pub fn new(state: Arc<LocalIdentityState>) -> Self {
        Self { state }
    }

    pub fn handle(&self, _query: GetLocalIdentity) -> Option<LocalIdentitySummary> {
        self.state.read(|identity| LocalIdentitySummary {
            peer: identity.peer_id(),
            display_name: identity.display_name().clone(),
            fingerprint: identity.fingerprint(),
        })
    }
}
