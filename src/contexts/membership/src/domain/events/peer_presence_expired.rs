use shared_types::PeerId;

use crate::domain::Millis;

/// A peer's evidence of life aged past the offline window, so the local view
/// now treats it as gone (canvas §2.2, invariant 7).
///
/// It carries **both** the evidence instant it was derived from and the instant
/// the derivation ran, because this event is a statement about the local view,
/// not about the remote peer. A peer that looks expired here may be perfectly
/// healthy behind a broken path; the payload shows the inputs so a diagnostic
/// can say so.
///
/// Distinct from `PeerDisconnected`: silence is not a closed session. Nothing
/// here means the transport reported anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerPresenceExpired {
    pub peer: PeerId,
    /// The last evidence of life the roster held.
    pub last_evidence_at: Millis,
    /// When the expiry was evaluated.
    pub at: Millis,
}
