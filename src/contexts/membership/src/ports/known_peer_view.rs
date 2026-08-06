use shared_types::PeerId;

use crate::domain::{
    Endpoint, KnownPeer, LivenessWindows, Millis, PeerStanding, Presence, SessionState,
};

/// One roster entry as a reader sees it: the read model of
/// [`MembershipQueryPort::known_peers`](crate::ports::MembershipQueryPort::known_peers).
///
/// # Why presence is a field here but not on [`KnownPeer`]
///
/// The roster stores *evidence*, never presence (invariant 7). A view is taken
/// at one instant, so it can carry the derivation that instant produced — and
/// it must, because the alternative is handing the caller a live entry and a
/// clock and hoping it derives the same thing. Two views taken at different
/// instants from an unchanged roster legitimately disagree; that is the point
/// of a derivation.
///
/// The view is owned and detached: nothing borrows from the roster, so a UI
/// can hold it across a redraw while the network keeps changing underneath.
///
/// # One classification, not two readings
///
/// [`standing`](Self::standing) is a pure function of
/// [`session`](Self::session) and [`presence`](Self::presence), both already
/// on this struct: no additional data crosses the port for it, and there is
/// nothing for it to disagree with. That is the whole of canvas D5 — the status
/// line counts the [`Linked`](PeerStanding::Linked) standings of the same
/// slice of views the roster renders, so `connected (n)` is by construction the
/// number of rows that show as linked rather than a second traversal with a
/// second predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeerView {
    pub peer: PeerId,
    /// Addresses this peer announced, oldest first.
    pub endpoints: Vec<Endpoint>,
    /// Presence as derived at the instant this view was taken.
    pub presence: Presence,
    /// When this peer last produced evidence of life, or `None` if it never
    /// has — the state every entry starts in, and the one
    /// [`Presence::Unknown`] reports. It is not a missing field to be filled
    /// in with the instant we were *told about* the peer: that instant is not
    /// evidence, and writing it here is what made a discovered peer look alive
    /// (canvas D1, D3).
    pub last_seen_at: Option<Millis>,
    /// The session held for this peer, if any. `None` means known but never
    /// dialled — discovery says where a peer is, not that it answers.
    pub session: Option<SessionState>,
}

impl KnownPeerView {
    /// Projects a roster entry as of `now`.
    pub fn of(entry: &KnownPeer, now: Millis, windows: LivenessWindows) -> Self {
        Self {
            peer: entry.peer(),
            endpoints: entry.endpoints().to_vec(),
            presence: entry.presence(now, windows),
            last_seen_at: entry.last_seen_at(),
            session: entry.session().map(crate::domain::Session::state),
        }
    }

    /// How this peer stands: the link and the evidence as one value.
    ///
    /// The single classification the status line and this row are both derived
    /// from (canvas D5). Derived on demand from fields this view already holds,
    /// so it is never a second encoding of them, and
    /// [`Linked(Offline)`](PeerStanding::Linked) survives to the renderer as a
    /// value of its own — a link that is up to a peer that is not answering,
    /// which is neither a contradiction to design away nor the same thing as a
    /// peer with no link at all (safeguard S4).
    pub const fn standing(&self) -> PeerStanding {
        PeerStanding::of(self.session, self.presence)
    }

    /// Whether an established session exists — the only sense in which this
    /// context calls a peer "connected".
    ///
    /// Deliberately a method rather than a field: a stored flag would be a
    /// second encoding of [`session`](Self::session), and two encodings of one
    /// fact are how a status line ends up lying. For the same reason it is
    /// answered by [`standing`](Self::standing) rather than by its own
    /// `matches!` — one predicate, so this can never disagree with the count.
    pub const fn is_connected(&self) -> bool {
        self.standing().is_linked()
    }
}
