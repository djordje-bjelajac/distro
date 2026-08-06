use crate::domain::{NetworkStatus, PeerStanding};
use crate::ports::KnownPeerView;

/// The status line and the roster rows as they were at one instant: the read
/// model of
/// [`MembershipQueryPort::network_view`](crate::ports::MembershipQueryPort::network_view)
/// (canvas §4, D5).
///
/// # Why the two halves travel together
///
/// They were fetched separately, and the screen contradicted itself:
/// `connected (2 peers)` above a roster in which every row read `offline`,
/// observed on two instances at once. Two calls give a caller two answers it
/// must then trust to agree — about a roster that changes underneath, against
/// two clock readings, through two predicates. Nothing forced them to agree,
/// and they did not.
///
/// # The count is not stored beside the rows; it is *read off* them
///
/// [`of`](Self::of) is the only way to state a count, and it derives one by
/// counting the [`Linked`](PeerStanding::Linked) standings of the very rows it
/// is handed. So `status.connected_peers()` is the number of rows that render
/// as linked — not because two computations were checked against each other,
/// but because there is only one. The fields are private for exactly this
/// reason: a `pub status` beside a `pub peers` is an invitation to set one
/// without the other, which is the shape of the original defect (canvas D5,
/// safeguard S4).
///
/// This is *not* an atomicity claim. Taking both readings from one snapshot is
/// hygiene — the contradiction was semantic and would have survived any number
/// of atomic reads.
///
/// # `Joining` claims nothing about any peer
///
/// [`joining`](Self::joining) is the one constructor that does not derive its
/// status from the rows, and it is not an exception to the rule above: `Joining`
/// is not a count. It says a bootstrap ladder is in flight, which no number of
/// sessions could tell the caller, and it asserts nothing about any row — every
/// row still carries its own standing. It outranks the count for the same
/// reason it does in
/// [`MembershipState`](crate::domain::NetworkStatus::Joining)'s older path: a
/// re-join over live sessions is still a join, and the in-flight operation is
/// what the caller is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkView {
    status: NetworkStatus,
    peers: Vec<KnownPeerView>,
}

impl NetworkView {
    /// Classifies one snapshot of rows: the status is the number of them that
    /// hold an established session.
    ///
    /// `peers` must be a single traversal of the roster taken at a single
    /// instant — that is what makes every row's presence comparable and the
    /// count a statement about these rows rather than about some other reading
    /// of the roster.
    pub fn of(peers: Vec<KnownPeerView>) -> Self {
        let standings = Self::standings_of(&peers);

        Self {
            status: NetworkStatus::from_standings(&standings),
            peers,
        }
    }

    /// The same snapshot, taken while a bootstrap ladder is in flight.
    pub const fn joining(peers: Vec<KnownPeerView>) -> Self {
        Self {
            status: NetworkStatus::Joining,
            peers,
        }
    }

    /// How connected this instance is, as of this snapshot.
    pub const fn status(&self) -> NetworkStatus {
        self.status
    }

    /// Every known peer, in `PeerId` order, each with the presence derived at
    /// the instant this snapshot was taken.
    ///
    /// Never-heard-from peers are included: they are dialable candidates, and
    /// hiding them turns "my peer vanished" into a support question. What they
    /// carry is [`Presence::Unknown`](crate::domain::Presence::Unknown), which a
    /// renderer states as nothing rather than as an absence (canvas §3).
    pub fn peers(&self) -> &[KnownPeerView] {
        &self.peers
    }

    /// How each row stands — the classification the status was counted from.
    pub fn standings(&self) -> Vec<PeerStanding> {
        Self::standings_of(&self.peers)
    }

    /// Hands the snapshot to a caller that wants to own the rows, a renderer
    /// most of all.
    pub fn into_parts(self) -> (NetworkStatus, Vec<KnownPeerView>) {
        (self.status, self.peers)
    }

    fn standings_of(peers: &[KnownPeerView]) -> Vec<PeerStanding> {
        peers.iter().map(KnownPeerView::standing).collect()
    }
}
