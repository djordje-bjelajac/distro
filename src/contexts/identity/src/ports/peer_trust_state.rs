use shared_types::{Fingerprint, PeerId};

use crate::domain::VerificationState;

/// Read model returned by
/// [`IdentityQueryPort::peer_trust_state`](crate::ports::IdentityQueryPort::peer_trust_state):
/// what this peer locally believes about one remote peer.
///
/// The two axes stay separate exactly as in
/// [`TrustRecord`](crate::domain::TrustRecord) — verification answers "is this
/// key really theirs?", blocking answers "do I want their traffic?" — so a
/// UI can render a blocked *and* verified peer without inventing a combined
/// state. The [`Fingerprint`] rides along so the verification prompt has the
/// digest to compare (AC6).
///
/// A peer that has never been verified or blocked has no stored record, and
/// this read model then reports the trust-on-first-use starting point
/// (`Unverified`, not blocked). Asking about a peer stores nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerTrustState {
    /// The remote peer this state is about.
    pub peer: PeerId,
    /// How far this peer has climbed the trust-on-first-use ladder.
    pub verification: VerificationState,
    /// Whether the local user is dropping this peer's traffic (invariant 11).
    pub blocked: bool,
    /// The digest to compare out-of-band before verifying (AC6).
    pub fingerprint: Fingerprint,
}
