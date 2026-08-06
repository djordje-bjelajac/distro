use shared_types::PeerDisconnected;

use crate::domain::events::NetworkLeft;
use crate::ports::PeerCacheError;

/// What a deliberate departure from the network did.
///
/// Leaving is a local decision, not an observation (see
/// [`NetworkLeft`]): losing the last session to a network failure is a
/// `PeerDisconnected` and a return to `Isolated`, never this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveOutcome {
    /// The departure event, published last so no consumer sees the network
    /// left before it sees the sessions end.
    pub left: NetworkLeft,
    /// One per established session that ended, in `PeerId` order. Sessions
    /// still handshaking produce nothing here — no `PeerConnected` was ever
    /// published for them, and an unmatched disconnect would make `messaging`
    /// fail directs for a peer it never considered reachable (D10).
    pub disconnected: Vec<PeerDisconnected>,
    /// How many peers were written to the cache for the next launch's first
    /// bootstrap rung (D1).
    ///
    /// Fewer than the roster holds whenever some entries never produced
    /// evidence of life: an identity this peer was only *told about* is not
    /// written to disk, where the next launch would dial it first (D8, S5).
    pub cached_peers: usize,
    /// Why the cache could not be written, when it could not be.
    ///
    /// Reported rather than raised: the departure itself succeeded, and the
    /// cost of a failed save is a colder start next time, not a failed leave.
    /// It is still stated, because a machine that silently stops warm-starting
    /// falls back to needing a join ticket and the user would have no idea why.
    pub cache_failure: Option<PeerCacheError>,
}
