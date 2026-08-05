use std::fmt;

use shared_types::PeerId;

/// Custody of the local peer's Ed25519 keypair (D5, AC9).
///
/// # Secret bytes never cross this port
///
/// The only thing that comes back is the **public** [`PeerId`]. The keypair
/// itself stays behind the adapter — which is also why signing is a separate
/// port ([`EnvelopeSignerPort`](crate::ports::EnvelopeSignerPort)) rather than
/// a method that hands out a key: no caller can ever hold, log, or serialise
/// the secret because no signature in this crate returns it.
///
/// # Load-or-create
///
/// [`load_or_create_local_peer`](Self::load_or_create_local_peer) is a single
/// idempotent operation, not a create-then-load pair: first launch generates
/// and persists a keypair with no user interaction (AC1), and every later
/// call returns that same identity (AC9). Two calls in one process must agree.
/// Implementations carry a schema version from v1 and must fail with
/// [`IdentityKeyStoreError::UnsupportedSchemaVersion`] rather than rewrite a
/// file they do not understand (S4).
pub trait IdentityKeyStorePort {
    /// Returns the local peer's identity, creating and persisting a keypair on
    /// first use.
    fn load_or_create_local_peer(&self) -> Result<PeerId, IdentityKeyStoreError>;
}

/// Typed failure of an [`IdentityKeyStorePort`] operation.
///
/// Deliberately coarse and free of I/O detail: the domain and application
/// layers decide what to do per variant, while adapters log the specifics they
/// alone can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKeyStoreError {
    /// Stored key material exists but could not be read.
    Unreadable,
    /// Stored key material was read but is not a usable keypair.
    Corrupt,
    /// The store carries a schema version this build does not understand; the
    /// original must be preserved untouched (S4).
    UnsupportedSchemaVersion { found: u32 },
    /// No key material existed and a new keypair could not be created or
    /// persisted.
    CreationFailed,
}

impl fmt::Display for IdentityKeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("local key store could not be read"),
            Self::Corrupt => f.write_str("local key store does not contain a usable keypair"),
            Self::UnsupportedSchemaVersion { found } => {
                write!(f, "local key store has unsupported schema version {found}")
            }
            Self::CreationFailed => f.write_str("local keypair could not be created"),
        }
    }
}

impl std::error::Error for IdentityKeyStoreError {}
