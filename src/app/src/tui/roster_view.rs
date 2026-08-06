use identity::domain::VerificationState;
use membership::domain::{PeerStanding, Presence};
use membership::ports::KnownPeerView;
use shared_types::PeerId;

use crate::composition::PeerTrust;
use crate::tui::PeerLabels;

/// One roster row, with everything the pane draws already decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub peer: PeerId,
    /// The leading fingerprint characters — never a name the peer chose
    /// (invariant 8).
    pub label: String,
    /// The link and the evidence as **one** value, derived at the instant the
    /// view was taken and never stored (invariant 7, canvas D5).
    ///
    /// This used to be two fields — a `presence` and a `connected` flag — and
    /// two fields are two things a writer can set independently. They were set
    /// from two reads of the roster at two instants, and the screen said
    /// `connected (2 peers)` above a roster in which every row read `offline`.
    /// A [`PeerStanding`] is the single classification the status line's count
    /// is derived from as well, so the row and the count cannot disagree:
    /// [`presence`](Self::presence) and [`connected`](Self::connected) are read
    /// off it rather than stored beside it.
    pub standing: PeerStanding,
    pub trust: PeerTrust,
    /// The link runs through a third peer acting as relay (AC12). Worth
    /// showing: it is the difference between a direct path and one that
    /// depends on a stranger staying online.
    pub relayed: bool,
}

impl RosterRow {
    /// What the evidence says about this peer, independent of the link.
    pub const fn presence(&self) -> Presence {
        self.standing.presence()
    }

    /// An established session exists. **Not** the same as online: a peer seen
    /// announcing itself a second ago is online with no session at all, and a
    /// peer holding a session goes stale and then offline if it stops
    /// speaking.
    pub const fn connected(&self) -> bool {
        self.standing.is_linked()
    }

    /// The presence cell: what this row says about the peer at the far end.
    ///
    /// # A never-heard-from peer says nothing at all
    ///
    /// [`Presence::Unknown`] renders as an **empty cell**, not as the word
    /// `unknown` that [`Presence`]'s `Display` yields for a diagnostic. After a
    /// cache load most rows are in this state, so a word here would be printed
    /// down the whole pane on every launch — and a column of `unknown` reads as
    /// a fault when the honest statement is that nothing has been heard yet.
    /// The blank is the rendering decision; `Display` stays the diagnostic
    /// label, and the two are deliberately different (canvas §3, D1).
    ///
    /// The row is still drawn: a never-heard-from peer is an address worth
    /// dialling, and hiding it turns "my peer vanished" into a support
    /// question. What it carries is its trust badge, its label, and — if a
    /// session is somehow held — its link mark. None of that claims a peer is
    /// present, which is the point.
    ///
    /// # A linked peer that has gone quiet says both halves
    ///
    /// `Linked(Offline)` renders `connected · not answering`. The session is
    /// up and the peer is not responding; both are true, and this is the state
    /// that produced the observed contradiction. Rendering the bare word
    /// `offline` here would put an absence beside a link the status line is
    /// counting — and the two ways of making that go away are both lies:
    /// dropping the peer from the count hides a link a direct message can
    /// still be attempted over, and calling it online asserts evidence nobody
    /// produced (canvas D5, safeguard S4). So it stays a state of its own with
    /// a string of its own, never the same string as `Unlinked(Offline)`.
    ///
    /// # Everything else reads as the evidence reads
    ///
    /// `online` and `stale` are the same words for a linked peer and an
    /// unlinked one: the link is already in its own column
    /// ([`link_mark`](Self::link_mark)), and `stale` is the honest "not known
    /// yet" rather than an absence, so neither needs qualifying. Those three
    /// words match [`Presence`]'s `Display` exactly, which a test pins so the
    /// two cannot drift apart unnoticed.
    pub const fn presence_text(&self) -> &'static str {
        match self.standing {
            // The state the screenshot was of. Never `"offline"`.
            PeerStanding::Linked(Presence::Offline) => "connected · not answering",
            PeerStanding::Linked(Presence::Unknown) | PeerStanding::Unlinked(Presence::Unknown) => {
                ""
            }
            PeerStanding::Linked(Presence::Online) | PeerStanding::Unlinked(Presence::Online) => {
                "online"
            }
            PeerStanding::Linked(Presence::Stale) | PeerStanding::Unlinked(Presence::Stale) => {
                "stale"
            }
            PeerStanding::Unlinked(Presence::Offline) => "offline",
        }
    }

    /// The badge for this peer's trust, in one column.
    ///
    /// The two axes are orthogonal and a peer can be both, so the badge shows
    /// both rather than collapsing them into a ladder the domain does not
    /// have:
    ///
    /// | badge | meaning |
    /// | --- | --- |
    /// | `?` | unverified — seen, but its key has never been confirmed out of band |
    /// | `✓` | verified — its fingerprint was compared and matched (AC6) |
    /// | `⊘` | blocked — its content is refused here, whatever its verification (invariant 11) |
    /// | `⊘✓` | blocked and verified: this is definitely them, and their traffic is still dropped |
    pub fn trust_badge(&self) -> &'static str {
        match (self.trust.blocked, self.trust.verification) {
            (true, VerificationState::Verified) => "⊘✓",
            (true, VerificationState::Unverified) => "⊘",
            (false, VerificationState::Verified) => "✓",
            (false, VerificationState::Unverified) => "?",
        }
    }

    /// The reachability mark: a live session, a relayed live session, or
    /// nothing.
    pub const fn link_mark(&self) -> &'static str {
        match (self.connected(), self.relayed) {
            (true, true) => "⇄",
            (true, false) => "→",
            (false, _) => " ",
        }
    }
}

/// Builds the roster pane's rows from one view of the roster and this peer's
/// trust snapshot.
///
/// # Nothing is filtered
///
/// Not blocked peers, not offline ones, and **not never-heard-from ones**. A
/// blocked peer that vanished from the roster would leave a user unable to
/// *un*block it; an offline peer is the only record that a peer exists at all —
/// `Offline` is a derivation about evidence age, not a statement that a peer is
/// gone (invariant 7); and a peer that has never been heard from is a dialable
/// candidate, so hiding it turns "my peer vanished" into a support question
/// (canvas §3). What a never-heard-from peer gets is a blank presence cell, not
/// omission: see [`RosterRow::presence_text`].
///
/// Order is the roster's own (`PeerId` order, per `MembershipQueryPort`), which
/// is stable across redraws and independent of arrival — a list that reordered
/// itself as peers spoke would be unusable.
///
/// `peers` is expected to be one snapshot — the rows of a single
/// `NetworkView` — because the status line's count is derived from the very
/// same standings these rows carry. Rows assembled from a second read would be
/// counting one roster and drawing another, which is the defect this whole
/// operation removes. [`NetworkPanes`](crate::tui::NetworkPanes) is the caller
/// that guarantees it.
pub fn roster_rows(
    peers: &[KnownPeerView],
    labels: PeerLabels,
    trust_of: impl Fn(PeerId) -> PeerTrust,
) -> Vec<RosterRow> {
    peers
        .iter()
        .map(|view| RosterRow {
            peer: view.peer,
            label: labels.label(view.peer),
            standing: view.standing(),
            trust: trust_of(view.peer),
            relayed: view
                .endpoints
                .iter()
                .any(membership::domain::Endpoint::is_relayed),
        })
        .collect()
}
