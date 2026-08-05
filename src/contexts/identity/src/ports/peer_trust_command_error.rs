use std::fmt;

use crate::domain::TrustRecordError;
use crate::ports::TrustRecordStoreError;

/// Typed failure of a trust-changing command
/// ([`block_peer`](crate::ports::IdentityCommandPort::block_peer),
/// [`unblock_peer`](crate::ports::IdentityCommandPort::unblock_peer)).
///
/// The two variants are different kinds of "no": [`Rejected`](Self::Rejected)
/// means the domain refused a transition that would change nothing — the
/// caller's view of this peer is stale — while [`Store`](Self::Store) means
/// the change may simply not have survived. Collapsing them would let a UI
/// report "blocked" for a write that never landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTrustCommandError {
    /// The trust record could not be read or written.
    Store(TrustRecordStoreError),
    /// The domain rejected the transition (already blocked / not blocked).
    Rejected(TrustRecordError),
}

impl From<TrustRecordStoreError> for PeerTrustCommandError {
    fn from(error: TrustRecordStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<TrustRecordError> for PeerTrustCommandError {
    fn from(error: TrustRecordError) -> Self {
        Self::Rejected(error)
    }
}

impl fmt::Display for PeerTrustCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Rejected(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PeerTrustCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Rejected(error) => Some(error),
        }
    }
}
