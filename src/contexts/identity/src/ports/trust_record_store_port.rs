use std::fmt;

use shared_types::PeerId;

use crate::domain::TrustRecord;

/// Custody of this peer's [`TrustRecord`]s — what it locally believes about
/// every remote peer it has verified or blocked (canvas §2.1).
///
/// # Why `identity` owns this trait
///
/// The domain has the aggregate but nothing that outlives a process, and both
/// halves of trust are meant to survive a restart: a fingerprint comparison a
/// user performed once must not have to be repeated, and a blocked peer must
/// stay blocked. The store is therefore an outbound port of this context,
/// hand-written here rather than derived from any framework, and
/// `infra-store-fs` implements it in **OP-11** alongside the keystore.
///
/// # The block list crosses no context boundary
///
/// Invariant 11 says a blocked peer's envelopes are dropped at the application
/// boundary of *every* context, and `messaging` enforces that through its own
/// `AuthorPolicyPort`. The composition root (OP-12) is what joins the two: it
/// reads the block list through this port — or through
/// [`IdentityQueryPort::blocked_peers`](crate::ports::IdentityQueryPort::blocked_peers)
/// — and hands it to `messaging`'s own trait. Neither context imports the
/// other and no port trait is ever published in `shared_types` (canvas §4).
/// Blocking stays purely local: nothing here is announced to the network.
///
/// # Contract
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn TrustRecordStorePort + Send + Sync>` and tests can substitute an
/// in-memory fake. An absent record is `Ok(None)`, never an error — an unknown
/// peer is the trust-on-first-use starting point, not a failure. Saving is a
/// whole-record upsert keyed by [`TrustRecord::peer`]. Implementations carry a
/// schema version from v1 and must fail with
/// [`TrustRecordStoreError::UnsupportedSchemaVersion`] rather than rewrite a
/// file they do not understand (S4).
pub trait TrustRecordStorePort {
    /// Returns the stored record for `peer`, or `None` if this peer has never
    /// been verified or blocked.
    fn load_trust_record(&self, peer: PeerId)
    -> Result<Option<TrustRecord>, TrustRecordStoreError>;

    /// Stores `record`, replacing any record held for the same peer.
    fn save_trust_record(&self, record: &TrustRecord) -> Result<(), TrustRecordStoreError>;

    /// Returns every peer whose record currently carries the blocked flag.
    ///
    /// Order is implementation-defined; callers that need a stable order sort
    /// for themselves (S5), as `ListBlockedPeersHandler` does.
    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError>;
}

/// Typed failure of a [`TrustRecordStorePort`] operation.
///
/// Deliberately coarse and free of I/O detail, matching
/// [`IdentityKeyStoreError`](crate::ports::IdentityKeyStoreError): the
/// application layer decides what to do per variant, while adapters log the
/// specifics they alone can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustRecordStoreError {
    /// Stored trust records exist but could not be read.
    Unreadable,
    /// Stored trust records were read but are not usable records.
    Corrupt,
    /// The store carries a schema version this build does not understand; the
    /// original must be preserved untouched (S4).
    UnsupportedSchemaVersion { found: u32 },
    /// A record could not be written; the caller must assume the change did
    /// not survive.
    WriteFailed,
}

impl fmt::Display for TrustRecordStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("trust record store could not be read"),
            Self::Corrupt => f.write_str("trust record store does not contain usable records"),
            Self::UnsupportedSchemaVersion { found } => {
                write!(
                    f,
                    "trust record store has unsupported schema version {found}"
                )
            }
            Self::WriteFailed => f.write_str("trust record could not be written"),
        }
    }
}

impl std::error::Error for TrustRecordStoreError {}
