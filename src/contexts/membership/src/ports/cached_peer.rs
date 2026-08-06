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
///
/// # `last_seen_at` stays non-optional
///
/// Only peers that have produced evidence are cached (canvas D8), so there is
/// always an instant to store and the persisted schema does not change — no
/// version bump and no migration (S9). The filter is not tidiness: the roster
/// is fed by mDNS and Kademlia, the cache is written to disk, and the *first*
/// bootstrap rung dials what it finds there next launch. Caching an identity
/// this peer was merely told about hands an attacker the head of the dial queue
/// (S5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPeer {
    pub peer: PeerId,
    pub endpoints: Vec<Endpoint>,
    pub last_seen_at: Millis,
}

impl CachedPeer {
    /// Projects a roster entry into its cacheable part, or `None` if the peer
    /// has never produced evidence of life.
    ///
    /// The `Option` is where canvas D8 is enforced: an entry with no evidence
    /// has no honest instant to store, so rather than inventing one — the
    /// fabrication this canvas exists to remove — there is simply no cache
    /// entry to be made. A caller cannot write an unproven identity to disk
    /// without first deciding what to do about the `None`.
    pub fn of(entry: &KnownPeer) -> Option<Self> {
        Some(Self {
            peer: entry.peer(),
            endpoints: entry.endpoints().to_vec(),
            last_seen_at: entry.last_seen_at()?,
        })
    }
}
