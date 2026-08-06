use crate::domain::{Presence, SessionState};

/// How one remote peer stands in the local view: whether a usable link is held,
/// and what the evidence says about the peer at the far end (canvas §2.2, D5).
///
/// # Why one value carries both facts
///
/// The status line and the roster row used to be derived separately — the count
/// from the session predicate, the row from the age of the evidence — and
/// nothing forced them to agree. They disagreed in the field: `connected (2
/// peers)` above a roster in which every row read `offline`, observed on two
/// instances at once.
///
/// Taking both readings from one snapshot would not have prevented that. The
/// contradiction was semantic rather than a race, and would have survived any
/// number of atomic reads. So both readings are derived from **this single
/// classification**: the count is the number of [`Linked`](Self::Linked)
/// standings and the row is the standing itself, which turns a disagreement
/// between them into an arithmetic error rather than a design possibility
/// (canvas D5).
///
/// # `Linked` is an established session, not a live peer
///
/// `Linked` means precisely what
/// [`NetworkStatus::Connected`](crate::domain::NetworkStatus::Connected)
/// counts: a session whose handshake has completed. Not `Connecting` — a dial
/// in flight can carry nothing yet — and deliberately *not* "live by evidence",
/// because a peer whose heartbeat reached us over somebody else's link cannot
/// be sent a direct message. Counting those would report `connected (5)` where
/// zero directs can be sent: the mirror of the observed lie, not its cure
/// (canvas D4).
///
/// # `Linked(Offline)` is a legitimate state, and must stay one
///
/// The link is up and the peer is not answering. Both halves are independently
/// true, and the pair is the most useful thing this context can say about a
/// peer whose process died with its socket still open. Making it
/// unrepresentable is achievable only by lying — either dropping the peer from
/// the count, which hides a link a direct message can still be attempted over,
/// or asserting `Online` from the link, which is the fabricated evidence this
/// canvas exists to remove (canvas D5, safeguard S4). It therefore has to stay
/// distinguishable from `Unlinked(Offline)` all the way to the screen.
///
/// # Derived, stored nowhere
///
/// Like [`Presence`], a standing is a function of the roster's state at one
/// instant, never a field a writer sets. No aggregate holds one, so none can
/// assert one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerStanding {
    /// An established session is held: this peer can be sent a direct message
    /// right now. These are what
    /// [`NetworkStatus::Connected`](crate::domain::NetworkStatus::Connected)
    /// counts.
    Linked(Presence),
    /// No established session — never dialled, still dialling, or closed.
    /// Whatever the evidence says, nothing can be sent to this peer directly.
    Unlinked(Presence),
}

impl PeerStanding {
    /// Classifies a peer from its session state and its derived presence.
    ///
    /// Pure and total. Every combination of the two inputs has exactly one
    /// answer, including the combinations that look contradictory, and there is
    /// no error case: neither input can be missing, and a peer with no session
    /// and no evidence is `Unlinked(Unknown)` — a statement, not a failure to
    /// classify.
    ///
    /// The link predicate is `Some(Established)` and nothing else, matching
    /// [`PeerRoster::established_session_count`](crate::domain::PeerRoster::established_session_count)
    /// exactly. `Connecting` is the trap it must not fall into: a connecting
    /// session is *live*, so a predicate written as "holds a live session"
    /// would count a dial in flight and report a peer as reachable before the
    /// handshake that makes it so.
    ///
    /// The match is exhaustive rather than a wildcard on purpose: a new
    /// [`SessionState`] must force a decision here instead of silently
    /// defaulting to unlinked.
    pub const fn of(session: Option<SessionState>, presence: Presence) -> Self {
        match session {
            Some(SessionState::Established) => Self::Linked(presence),
            Some(SessionState::Connecting | SessionState::Closed) | None => {
                Self::Unlinked(presence)
            }
        }
    }

    /// Whether an established session is held — the predicate
    /// [`NetworkStatus::from_standings`](crate::domain::NetworkStatus::from_standings)
    /// counts.
    pub const fn is_linked(self) -> bool {
        matches!(self, Self::Linked(_))
    }

    /// What the evidence says about the peer, independent of the link.
    ///
    /// The standing adds the link to the presence rather than replacing it: a
    /// renderer that needed a second lookup to find the presence would be back
    /// to two sources for one row, which is where the two stories diverged.
    pub const fn presence(self) -> Presence {
        match self {
            Self::Linked(presence) | Self::Unlinked(presence) => presence,
        }
    }
}
