use shared_types::PeerId;

/// A remote peer was blocked locally (canvas invariant 11).
///
/// Blocking is purely local: nothing is announced to the network and the
/// blocked peer is never told. It does not touch verification state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerBlocked {
    pub peer: PeerId,
}
