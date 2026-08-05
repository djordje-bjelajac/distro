use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::{IdentityKeyStoreError, IdentityKeyStorePort, LocalIdentityAssumption};

/// Assume this process's persistent identity, generating the keypair if this
/// is the first launch.
///
/// `display_name` of `None` means "derive one from the peer's own
/// fingerprint": first launch must work with no configuration, no account, and
/// no prompt (AC1), so a missing name can never become a question to the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InitializeLocalIdentity {
    /// The name to start with, or `None` to derive one.
    pub display_name: Option<DisplayName>,
}

impl InitializeLocalIdentity {
    /// The zero-interaction first launch (AC1).
    pub const fn with_derived_display_name() -> Self {
        Self { display_name: None }
    }

    /// Start with a name the user (or a restored setting) already supplied.
    pub const fn named(display_name: DisplayName) -> Self {
        Self {
            display_name: Some(display_name),
        }
    }
}

/// Handles [`InitializeLocalIdentity`]: the load-or-create bootstrap that
/// AC1 and AC9 both rest on.
///
/// # Idempotent in two different senses
///
/// *Across launches* the keystore is idempotent: it creates a keypair the
/// first time and loads that same one every later time, so the `PeerId` is
/// stable across restarts (AC9) and this handler cannot tell the two cases
/// apart — by design, since the observable outcome is identical.
///
/// *Within one process* this handler is idempotent: a second call reports
/// [`LocalIdentityAssumption::AlreadyAssumed`], reads no store, and emits no
/// second event. That matters because re-issuing the command must not reset a
/// display name the user has since set, and because a duplicated
/// `LocalIdentityInitialized` would tell subscribers a peer changed identity
/// when nothing happened.
///
/// A keystore failure is returned as [`IdentityKeyStoreError`] and leaves the
/// state untouched: no panic, no half-assumed identity, and a later attempt
/// can still succeed once the underlying problem is fixed.
#[derive(Clone)]
pub struct InitializeLocalIdentityHandler {
    state: Arc<LocalIdentityState>,
    key_store: Arc<dyn IdentityKeyStorePort + Send + Sync>,
}

impl InitializeLocalIdentityHandler {
    pub fn new(
        state: Arc<LocalIdentityState>,
        key_store: Arc<dyn IdentityKeyStorePort + Send + Sync>,
    ) -> Self {
        Self { state, key_store }
    }

    pub fn handle(
        &self,
        command: InitializeLocalIdentity,
    ) -> Result<LocalIdentityAssumption, IdentityKeyStoreError> {
        self.state.assume_once(|| {
            let peer = self.key_store.load_or_create_local_peer()?;
            let display_name = command
                .display_name
                .unwrap_or_else(|| DisplayName::derived_from(&peer));

            Ok(LocalIdentity::initialize(peer, display_name))
        })
    }
}
