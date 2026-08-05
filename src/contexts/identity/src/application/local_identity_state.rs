use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::LocalIdentity;
use crate::domain::events::LocalIdentityInitialized;
use crate::ports::LocalIdentityAssumption;

/// The one [`LocalIdentity`] this process is running as, from before it is
/// assumed until the process ends.
///
/// # Why the application layer holds it
///
/// `IdentityKeyStorePort` persists the keypair — that is what AC9 pins — but
/// nothing in v1 persists the *display name*, and the canvas declares no port
/// that would (§4). The name is therefore process-scoped: a restart derives it
/// again from the peer's fingerprint unless the user sets one, which is why
/// [`DisplayName::derived_from`](crate::domain::DisplayName::derived_from) has
/// to be stable rather than random. The identity itself is not lost — it is
/// re-assumed from the keystore on every launch.
///
/// Held here rather than inside one handler because the command and query
/// sides both need it and must see the same peer: a rename issued through
/// `IdentityCommandPort` has to be visible through `IdentityQueryPort`.
/// [`IdentityContext`](crate::application::IdentityContext) is what guarantees
/// they share one instance.
///
/// # Interior mutability, not `RefCell`
///
/// A composition root drives this context from more than one task, so the cell
/// must be `Sync`; `Mutex` is the std answer and this crate takes no async
/// runtime dependency. A poisoned lock is recovered rather than propagated —
/// an assertion failure elsewhere must not turn every later identity read into
/// a panic — and no lock is ever held across a call into another lock.
pub struct LocalIdentityState {
    identity: Mutex<Option<LocalIdentity>>,
}

impl LocalIdentityState {
    /// The state of a process that has not yet run `InitializeLocalIdentity`.
    pub const fn uninitialized() -> Self {
        Self {
            identity: Mutex::new(None),
        }
    }

    /// Installs the identity `assume` produces, but only if none is installed
    /// yet.
    ///
    /// This is where the idempotency of `InitializeLocalIdentity` lives. The
    /// lock is deliberately held across `assume`: two callers racing to
    /// bootstrap must end with one identity and exactly one
    /// [`LocalIdentityInitialized`], and the loser must not perform the
    /// keystore read at all. `assume` is not called when an identity is
    /// already installed, and a failing `assume` installs nothing — there is
    /// no half-assumed state to observe.
    pub(crate) fn assume_once<E>(
        &self,
        assume: impl FnOnce() -> Result<(LocalIdentity, LocalIdentityInitialized), E>,
    ) -> Result<LocalIdentityAssumption, E> {
        let mut slot = self.lock();

        if let Some(existing) = slot.as_ref() {
            return Ok(LocalIdentityAssumption::AlreadyAssumed(existing.peer_id()));
        }

        let (identity, event) = assume()?;
        *slot = Some(identity);
        Ok(LocalIdentityAssumption::Assumed(event))
    }

    /// Reads the installed identity, or `None` if there is none yet.
    ///
    /// `view` returns an owned value: nothing borrowed from the identity may
    /// outlive the lock.
    pub(crate) fn read<R>(&self, view: impl FnOnce(&LocalIdentity) -> R) -> Option<R> {
        self.lock().as_ref().map(view)
    }

    /// Changes the installed identity, or reports `None` if there is none yet.
    ///
    /// Never creates an identity: bootstrapping is
    /// [`assume_once`](Self::assume_once)'s job alone, so a rename can never
    /// invent a peer.
    pub(crate) fn modify<R>(&self, change: impl FnOnce(&mut LocalIdentity) -> R) -> Option<R> {
        self.lock().as_mut().map(change)
    }

    fn lock(&self) -> MutexGuard<'_, Option<LocalIdentity>> {
        self.identity.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for LocalIdentityState {
    fn default() -> Self {
        Self::uninitialized()
    }
}
