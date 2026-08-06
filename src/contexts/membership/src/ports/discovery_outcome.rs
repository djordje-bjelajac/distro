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
/// same peer is re-announced continually and its addresses are merged, and no
/// event is published because nothing new was learned. It refreshes **no
/// evidence of life** — a re-announcement is still a third party's claim
/// (invariant 2, canvas D3).
///
/// # Why a full roster is a state here and not an error
///
/// [`RosterFull`](Self::RosterFull) is the same judgement as
/// [`OwnAnnouncement`](Self::OwnAnnouncement), for a sharper reason. The roster
/// is capped because entries arrive from the network (canvas D9), so reaching
/// the cap is an ordinary condition under load rather than a fault by the
/// adapter that reported the sighting — there is nothing it could have done
/// differently, and nothing it can do about it.
///
/// Reporting it as `Err` would be actively harmful. Discovery is loud: with
/// Kademlia feeding sightings, a node whose roster is full would answer *every*
/// subsequent announcement with an error, so a caller that logs errors emits an
/// unbounded stream of them exactly when the node is busiest. Worse, that
/// stream is attacker-inducible by the same party the cap defends against
/// (safeguard S5) — a bounded-memory defence would have bought an unbounded
/// diagnostic flood. A state costs nothing and says the same thing.
///
/// The contrast is [`PeerRosterError::NoEndpoints`](crate::domain::PeerRosterError::NoEndpoints),
/// which stays an error because it *is* a caller mistake: an adapter that
/// reports a peer with nowhere to reach it has reported nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryOutcome {
    /// First sighting: the peer entered the roster and the event says so.
    Recorded(PeerDiscovered),
    /// The peer was already known; addresses merged, no event, and no evidence
    /// of life — being announced again is not the peer speaking.
    Refreshed,
    /// The announcement named the local peer itself; nothing was recorded.
    OwnAnnouncement,
    /// The roster is at
    /// [`MAX_PEERS`](crate::domain::PeerRoster::MAX_PEERS) and every entry has a
    /// session or has produced evidence, so there was no unproven entry to
    /// evict for this one. The peer was **not** recorded, and that is a
    /// statement rather than a failure — see this type's documentation.
    RosterFull,
}

impl DiscoveryOutcome {
    /// The event this observation produced, or `None` when it produced none.
    pub const fn event(&self) -> Option<&PeerDiscovered> {
        match self {
            Self::Recorded(event) => Some(event),
            Self::Refreshed | Self::OwnAnnouncement | Self::RosterFull => None,
        }
    }

    /// Whether the roster gained an entry.
    pub const fn is_new_peer(&self) -> bool {
        matches!(self, Self::Recorded(_))
    }

    /// Whether the roster now holds this peer — false only when it was refused.
    ///
    /// The distinction [`is_new_peer`](Self::is_new_peer) cannot make: a caller
    /// about to dial a sighting needs to know the address was kept, and
    /// [`Refreshed`](Self::Refreshed) and [`Recorded`](Self::Recorded) both mean
    /// it was.
    pub const fn is_known(&self) -> bool {
        matches!(self, Self::Recorded(_) | Self::Refreshed)
    }
}
