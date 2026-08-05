use std::sync::Arc;

use membership::domain::{Endpoint, JoinTicket};
use membership::ports::{DiscoveredPeer, PeerDiscoveryError, PeerDiscoveryPort};
use shared_types::PeerId;

use crate::fabric::SimFabric;

/// One peer's `PeerDiscoveryPort` over the shared fabric.
///
/// The three methods are the three rungs of D1's bootstrap ladder seen from the
/// outside, and the fabric models each faithfully:
///
/// * [`announce`](PeerDiscoveryPort::announce) publishes this peer on its LAN
///   segment. Joining is public by construction (S8).
/// * [`observe_peers`](PeerDiscoveryPort::observe_peers) sees only the same
///   segment — which is what makes AC2 (two instances on one LAN) and AC3 (an
///   instance with no neighbour) two topologies rather than two mocks.
/// * [`redeem_join_ticket`](PeerDiscoveryPort::redeem_join_ticket) crosses
///   segments, because that is the entire reason the third rung exists.
///
/// # Nothing here reaches an operator-run host (S1)
///
/// There is no default rendezvous, bootstrap, or STUN address in this crate to
/// reach. Every peer a scenario can discover is a peer the scenario created.
pub struct SimPeerDiscovery {
    peer: PeerId,
    fabric: Arc<SimFabric>,
}

impl SimPeerDiscovery {
    /// A discovery mechanism speaking for `peer`.
    pub const fn new(peer: PeerId, fabric: Arc<SimFabric>) -> Self {
        Self { peer, fabric }
    }
}

impl PeerDiscoveryPort for SimPeerDiscovery {
    fn announce(&self, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        self.fabric.announce(self.peer, endpoints)
    }

    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        self.fabric.observe(self.peer)
    }

    fn redeem_join_ticket(
        &self,
        ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        // The ticket's expiry and protocol compatibility are deliberately not
        // re-checked: that is `JoinTicket::validate`, a pure domain rule the
        // application applies first. Checking it here as well would put a clock
        // on both sides of the boundary and let the two disagree.
        self.fabric.redeem(self.peer, ticket)
    }
}
