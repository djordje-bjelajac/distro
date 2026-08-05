use crate::domain::events::PeerDiscovered;

/// What recording one discovery observation changed.
///
/// # Why the local peer's own announcement is not an error
///
/// A gossiping network hands every peer its own announcement back, and a join
/// ticket can be pasted into the machine that minted it. Reporting those as
/// `Err` would make an adapter treat the most ordinary event on the wire as a
/// fault to log, so [`OwnAnnouncement`](Self::OwnAnnouncement) states plainly
/// that nothing was recorded and nothing was wrong. Invariant 2 still holds —
/// the roster never gains an entry for the local peer — it is simply enforced
/// without crying wolf.
///
/// [`Refreshed`](Self::Refreshed) is the common case in a running network: the
/// same peer is re-announced continually, its addresses are merged and its
/// evidence of life renewed, and no event is published because nothing new was
/// learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryOutcome {
    /// First sighting: the peer entered the roster and the event says so.
    Recorded(PeerDiscovered),
    /// The peer was already known; addresses merged, evidence refreshed, no
    /// event.
    Refreshed,
    /// The announcement named the local peer itself; nothing was recorded.
    OwnAnnouncement,
}

impl DiscoveryOutcome {
    /// The event this observation produced, or `None` when it produced none.
    pub const fn event(&self) -> Option<&PeerDiscovered> {
        match self {
            Self::Recorded(event) => Some(event),
            Self::Refreshed | Self::OwnAnnouncement => None,
        }
    }

    /// Whether the roster gained an entry.
    pub const fn is_new_peer(&self) -> bool {
        matches!(self, Self::Recorded(_))
    }
}
