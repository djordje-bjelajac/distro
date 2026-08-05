use shared_types::PeerId;

use crate::domain::{Endpoint, KnownPeer, LivenessWindows, Millis, Presence, SessionState};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeerView {
    pub peer: PeerId,
    /// Addresses this peer announced, oldest first.
    pub endpoints: Vec<Endpoint>,
    /// Presence as derived at the instant this view was taken.
    pub presence: Presence,
    /// When this peer last produced evidence of life.
    pub last_seen_at: Millis,
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

    /// Whether an established session exists — the only sense in which this
    /// context calls a peer "connected".
    ///
    /// Deliberately a method rather than a field: a stored flag would be a
    /// second encoding of [`session`](Self::session), and two encodings of one
    /// fact are how a status line ends up lying.
    pub const fn is_connected(&self) -> bool {
        matches!(self.session, Some(SessionState::Established))
    }
}
