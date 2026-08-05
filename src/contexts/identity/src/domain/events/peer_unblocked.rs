use shared_types::PeerId;

/// A previously blocked remote peer was unblocked locally.
///
/// The peer returns to whatever verification state it held before it was
/// blocked: blocking never discarded that state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerUnblocked {
    pub peer: PeerId,
}
