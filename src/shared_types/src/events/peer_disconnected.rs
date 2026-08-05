use crate::PeerId;

/// The authenticated session to `peer` ended, for any reason (canvas §2.2).
///
/// Published by `membership`; e.g. `messaging` reacts by failing that peer's
/// pending direct messages (D10). Carries the [`PeerId`] only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerDisconnected {
    pub peer: PeerId,
}
