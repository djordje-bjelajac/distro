use shared_types::PeerId;

/// A remote peer's key was confirmed out-of-band and its trust record moved
/// from `Unverified` to `Verified` (canvas §2.1, D5 TOFU).
///
/// Emitted only on the transition, never on a repeated confirmation of an
/// already-verified peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerVerified {
    pub peer: PeerId,
}
