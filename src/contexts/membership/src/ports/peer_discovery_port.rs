use std::fmt;

use crate::domain::{Endpoint, JoinTicket};
use crate::ports::DiscoveredPeer;

/// How this peer finds others and lets itself be found (canvas §4, D1).
///
/// The three methods are the three rungs of the bootstrap ladder seen from the
/// outside: [`observe_peers`](Self::observe_peers) covers whatever the
/// mechanism found on its own (LAN mDNS, the DHT, other peers' gossip),
/// [`redeem_join_ticket`](Self::redeem_join_ticket) is the out-of-band rung for
/// a first-ever internet join, and [`announce`](Self::announce) is this peer
/// paying the same service forward — every instance offers discovery to others
/// (AC4).
///
/// Nothing here may reach an operator-run host (S1). An implementation that
/// contacted a default rendezvous, bootstrap, or STUN server would satisfy the
/// signature and violate the requirement the whole design exists for.
pub trait PeerDiscoveryPort {
    /// Publishes the local peer's reachable endpoints so others can find it.
    ///
    /// Announcing is public by construction (S8): joining tells the network
    /// where this peer is.
    fn announce(&self, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError>;

    /// Reports the peers the mechanism has seen since the last call.
    ///
    /// An empty result is success, not failure — a LAN with no neighbour is
    /// the ordinary state of a first launch, and `Isolated` is a normal
    /// status.
    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError>;

    /// Dials the endpoints carried by `ticket` and reports the peer that
    /// answered.
    ///
    /// Implementations do **not** re-check the ticket's expiry or protocol
    /// version: that is [`JoinTicket::validate`], a pure domain rule the
    /// application applies first. Checking it here as well would put a clock
    /// on both sides of the boundary and let the two disagree.
    fn redeem_join_ticket(&self, ticket: &JoinTicket)
    -> Result<DiscoveredPeer, PeerDiscoveryError>;
}

/// Typed failure of a [`PeerDiscoveryPort`] operation.
///
/// Coarse and free of transport detail: callers decide what to do per variant
/// while the adapter logs the specifics only it can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerDiscoveryError {
    /// The discovery mechanism is not running.
    Unavailable,
    /// The local peer's announcement was refused.
    AnnouncementRejected,
    /// No endpoint in the ticket answered; the issuer may be offline or have
    /// moved (AC3 — a visible diagnostic, never a hang).
    TicketUnreachable,
}

impl fmt::Display for PeerDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("peer discovery is not available"),
            Self::AnnouncementRejected => f.write_str("the local peer's announcement was rejected"),
            Self::TicketUnreachable => f.write_str("no endpoint in the join ticket answered"),
        }
    }
}

impl std::error::Error for PeerDiscoveryError {}
