use shared_types::PeerId;

use crate::domain::{Endpoint, KnownPeer, Millis};

/// One peer as it survives a restart: the first rung of the D1 bootstrap
/// ladder.
///
/// Deliberately **not** a [`KnownPeer`]: sessions and derived presence do not
/// survive a process, and caching either would make a peer look connected on
/// the next launch before anything had been dialled. What persists is the pair
/// that makes a peer dialable again — its identity and its last known
/// addresses — plus the instant it was last seen, which is what lets a cache
/// prune entries that have been dead for months.
///
/// The stored form carries a schema version and rejects unknown ones (S4);
/// that is the adapter's concern (OP-11), not this struct's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPeer {
    pub peer: PeerId,
    pub endpoints: Vec<Endpoint>,
    pub last_seen_at: Millis,
}

impl CachedPeer {
    /// Projects a roster entry into its cacheable part.
    pub fn of(entry: &KnownPeer) -> Self {
        Self {
            peer: entry.peer(),
            endpoints: entry.endpoints().to_vec(),
            last_seen_at: entry.last_seen_at(),
        }
    }
}
