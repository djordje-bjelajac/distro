use identity::domain::VerificationState;
use membership::domain::Presence;
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
    /// Derived at the instant the view was taken, never stored (invariant 7).
    pub presence: Presence,
    /// An established session exists. **Not** the same as online: a peer seen
    /// announcing itself a second ago is online with no session at all.
    pub connected: bool,
    pub trust: PeerTrust,
    /// The link runs through a third peer acting as relay (AC12). Worth
    /// showing: it is the difference between a direct path and one that
    /// depends on a stranger staying online.
    pub relayed: bool,
}

impl RosterRow {
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
        match (self.connected, self.relayed) {
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
/// Not blocked peers, and not offline ones. A blocked peer that vanished from
/// the roster would leave a user unable to *un*block it, and an offline peer is
/// the only record that a peer exists at all — `Offline` is a derivation about
/// evidence age, not a statement that a peer is gone (invariant 7).
///
/// Order is the roster's own (`PeerId` order, per `MembershipQueryPort`), which
/// is stable across redraws and independent of arrival — a list that reordered
/// itself as peers spoke would be unusable.
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
            presence: view.presence,
            connected: view.is_connected(),
            trust: trust_of(view.peer),
            relayed: view
                .endpoints
                .iter()
                .any(membership::domain::Endpoint::is_relayed),
        })
        .collect()
}
