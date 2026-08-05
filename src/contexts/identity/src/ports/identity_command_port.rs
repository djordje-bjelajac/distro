use shared_types::PeerId;

use crate::domain::DisplayName;
use crate::domain::events::{DisplayNameChanged, PeerBlocked, PeerUnblocked, PeerVerified};
use crate::ports::{
    IdentityKeyStoreError, LocalIdentityAssumption, PeerTrustCommandError, SetDisplayNameError,
    TrustRecordStoreError,
};

/// The **inbound** (driving) contract of `identity`: everything that changes
/// this context's state (canvas §4, inbound column).
///
/// The composition root (OP-12) drives the context through this trait — a TUI
/// action, a startup step — and never reaches into a handler directly. Every
/// method here mutates; nothing here reads for display, which is
/// [`IdentityQueryPort`](crate::ports::IdentityQueryPort)'s job. Keeping the
/// two apart at the trait level is what makes the CQRS split visible to the
/// caller instead of merely being a convention inside the crate.
///
/// # Why the arguments are domain types, not command structs
///
/// `ports` may depend on `domain` and `shared_types` only, so a port signature
/// cannot name an application-layer type. The imperative command DTOs
/// (`InitializeLocalIdentity`, `SetDisplayName`, …) therefore live in
/// `application/commands/`, and the service implementing this trait builds
/// them from these arguments. The dependency keeps pointing inward.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn IdentityCommandPort + Send + Sync>`.
pub trait IdentityCommandPort {
    /// Assumes this process's persistent identity, creating and persisting a
    /// keypair on first launch and loading it on every later one (AC1, AC9).
    ///
    /// Idempotent: issuing it again in the same process reports
    /// [`LocalIdentityAssumption::AlreadyAssumed`], touches no store, and
    /// emits no second event. `display_name` of `None` means "derive one" —
    /// first launch asks the user nothing.
    fn initialize_local_identity(
        &self,
        display_name: Option<DisplayName>,
    ) -> Result<LocalIdentityAssumption, IdentityKeyStoreError>;

    /// Renames the local peer, validating `requested` through
    /// [`DisplayName`].
    ///
    /// Returns `Ok(None)` when the peer already has that name: nothing
    /// changed, so nothing is announced.
    fn set_display_name(
        &self,
        requested: &str,
    ) -> Result<Option<DisplayNameChanged>, SetDisplayNameError>;

    /// Records an out-of-band fingerprint confirmation for `peer` (D5).
    ///
    /// Idempotent, matching the domain: re-verifying an already verified peer
    /// succeeds with `Ok(None)` and writes nothing.
    fn verify_peer(&self, peer: PeerId) -> Result<Option<PeerVerified>, TrustRecordStoreError>;

    /// Blocks `peer` locally; its verification state is left untouched.
    ///
    /// Rejects with [`PeerTrustCommandError::Rejected`] when the peer is
    /// already blocked — the command would change nothing, and the caller's
    /// view is stale.
    fn block_peer(&self, peer: PeerId) -> Result<PeerBlocked, PeerTrustCommandError>;

    /// Unblocks `peer`, which returns to the verification state it kept
    /// throughout.
    ///
    /// Rejects with [`PeerTrustCommandError::Rejected`] when the peer is not
    /// blocked.
    fn unblock_peer(&self, peer: PeerId) -> Result<PeerUnblocked, PeerTrustCommandError>;
}
