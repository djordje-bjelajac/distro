use shared_types::PeerId;

use crate::domain::DisplayName;

/// The local peer assumed its persistent identity for this process.
///
/// Emitted once per `LocalIdentity` construction, whether the keypair was
/// created on first launch or loaded from an existing keystore — the domain
/// cannot tell the two apart, and by AC9 the observable outcome is the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalIdentityInitialized {
    pub peer: PeerId,
    pub display_name: DisplayName,
}
