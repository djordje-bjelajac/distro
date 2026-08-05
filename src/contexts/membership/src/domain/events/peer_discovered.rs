use shared_types::PeerId;

use crate::domain::Millis;

/// A peer entered this peer's roster for the first time (canvas §2.2).
///
/// Emitted only on first sight. Discovery repeats constantly in a gossiping
/// network — the same peer arrives from mDNS, from the DHT, and from other
/// peers' announcements — and re-announcing each one would drown any consumer
/// in events that carry no news.
///
/// Discovery says nothing about reachability: the peer is known, not connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerDiscovered {
    pub peer: PeerId,
    pub at: Millis,
}
