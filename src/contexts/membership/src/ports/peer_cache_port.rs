use std::fmt;

use crate::ports::CachedPeer;

/// Persistence for the known-peer set that makes the *next* launch a warm start
/// (canvas §4, D1).
///
/// This is the rung that makes the join-ticket cost a one-time one: after a
/// first successful join, a machine bootstraps from its own cache and needs no
/// ticket and no LAN neighbour ever again. It is also the only state this
/// context persists — conversation history does not (D7).
pub trait PeerCachePort {
    /// Loads the cached peers.
    ///
    /// An empty result is a cold start, not an error: it is exactly the case
    /// the rest of the bootstrap ladder exists for.
    fn load(&self) -> Result<Vec<CachedPeer>, PeerCacheError>;

    /// Replaces the cached set with `peers`.
    ///
    /// Replace rather than merge: the roster is the whole truth about known
    /// peers, and an append-only cache could never forget one.
    fn save(&self, peers: &[CachedPeer]) -> Result<(), PeerCacheError>;
}

/// Typed failure of a [`PeerCachePort`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerCacheError {
    /// The cache exists but could not be read.
    Unreadable,
    /// The cache was read but does not contain a usable peer set.
    Corrupt,
    /// The cache carries a schema version this build does not understand; the
    /// original must be preserved untouched rather than rewritten (S4).
    UnsupportedSchemaVersion { found: u32 },
    /// The cache could not be written.
    WriteFailed,
}

impl fmt::Display for PeerCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => f.write_str("peer cache could not be read"),
            Self::Corrupt => f.write_str("peer cache does not contain a usable peer set"),
            Self::UnsupportedSchemaVersion { found } => {
                write!(f, "peer cache has unsupported schema version {found}")
            }
            Self::WriteFailed => f.write_str("peer cache could not be written"),
        }
    }
}

impl std::error::Error for PeerCacheError {}
