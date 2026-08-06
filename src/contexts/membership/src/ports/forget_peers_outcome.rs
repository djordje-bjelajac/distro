use std::fmt;

use shared_types::PeerDisconnected;

use crate::ports::{EventPublisherError, PeerCacheError};

/// What forgetting every known peer did (canvas `0013`, D8).
///
/// # Why the cache failure is carried rather than raised
///
/// The operation is non-atomic in exactly one direction. The roster cannot
/// fail to empty — it is a map and a `clear` — while the cache is a file and
/// can refuse. Folding that into an `Err` would throw away the true half: the
/// peers *are* forgotten for this process, and the user is owed both facts,
/// because "forgot 12 peers" and "forgot 12 peers, but they will be back next
/// launch" are different situations with different next actions.
///
/// [`LeaveOutcome`](crate::ports::LeaveOutcome) carries its cache failure the
/// same way and for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgetPeersOutcome {
    /// How many roster entries were dropped.
    ///
    /// The roster's count, not the cache's: entries that had never produced
    /// evidence were never written to disk (D8 of the system canvas), so this
    /// is larger than the number of peers the file held whenever the roster
    /// had been told about a peer it never reached.
    pub forgotten: usize,
    /// One per established session that was closed on the way, in `PeerId`
    /// order — the departures announced before anything was forgotten.
    ///
    /// Sessions still handshaking produce nothing here, exactly as they
    /// produce nothing in a leave: no `PeerConnected` was ever published for
    /// them, and an unmatched disconnect would make `messaging` fail directs
    /// for a peer it never considered reachable.
    pub disconnected: Vec<PeerDisconnected>,
    /// Why the emptied cache could not be written, when it could not be.
    ///
    /// This is the failure that matters most to state plainly: the roster is
    /// empty and the screen says so, but the file on disk still holds the old
    /// peers and the next launch will warm-start from them. A user told only
    /// "forgotten" would have no way to know that.
    pub cache_failure: Option<PeerCacheError>,
}

/// Why forgetting could not be carried out at all.
///
/// Distinct from [`MembershipCommandError`](crate::ports::MembershipCommandError),
/// whose three variants describe a *roster*, *transport* or *publisher*
/// refusal and whose doc states that the peer cache is deliberately absent.
/// That reasoning is still correct, and neither of these two cases fits it:
/// one is a refusal to start, the other is the one failure that leaves
/// consumers behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetPeersError {
    /// A bootstrap ladder is running, so nothing was forgotten.
    ///
    /// The ladder reads the cache on its own thread and dials what it finds. A
    /// forget landing between that read and the dial would produce a join from
    /// peers the user had just erased — the roster refilling itself from state
    /// that no longer exists, which is precisely the confusion this operation
    /// is meant to end. Refusing is a stated outcome, not a silent no-op:
    /// leaving the user to wonder whether it worked is worse than saying no.
    JoinInFlight,
    /// A departure was made but could not be announced.
    ///
    /// The one case that leaves the roster ahead of its consumers, which is
    /// why it is not folded into the outcome beside the cache failure.
    Publisher(EventPublisherError),
}

impl fmt::Display for ForgetPeersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JoinInFlight => {
                f.write_str("peers cannot be forgotten while a join is in flight")
            }
            Self::Publisher(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ForgetPeersError {}

impl From<EventPublisherError> for ForgetPeersError {
    fn from(error: EventPublisherError) -> Self {
        Self::Publisher(error)
    }
}
