use crate::PeerId;

/// An authenticated session to `peer` was established (canvas §2.2).
///
/// Published by `membership`; e.g. `messaging` reacts by allowing direct
/// sends to the peer. Carries the [`PeerId`] only — deliberately nothing
/// about endpoints, transports, or session internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerConnected {
    pub peer: PeerId,
}
