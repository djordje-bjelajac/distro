use membership::domain::{Endpoint, JoinTicket};
use membership::ports::{DiscoveredPeer, PeerDiscoveryError, PeerDiscoveryPort};

use crate::runtime::NetworkHandle;
use crate::swarm::network_command::NetworkCommand;

/// `membership`'s `PeerDiscoveryPort` over the real swarm — D1's bootstrap
/// ladder as three methods.
///
/// * [`announce`](PeerDiscoveryPort::announce) publishes this peer's addresses
///   as external addresses, which identify then hands to every peer it meets
///   and Kademlia stores for peers that ask. Joining is public by construction
///   (S8).
/// * [`observe_peers`](PeerDiscoveryPort::observe_peers) drains what mDNS
///   (rung b), identify, and the DHT have seen since the last call. An empty
///   result is success: a LAN with no neighbour is the ordinary state of a
///   first launch, and `Isolated` is a normal status.
/// * [`redeem_join_ticket`](PeerDiscoveryPort::redeem_join_ticket) dials the
///   ticket's endpoints — rung c, the one that makes an internet-wide first
///   contact possible without an operator (AC3).
///
/// # Nothing here reaches a host somebody else operates (S1)
///
/// There is no default rendezvous, bootstrap, or STUN address in this build to
/// reach; see [`DistroBehaviour`](crate::swarm::distro_behaviour::DistroBehaviour)
/// for the list of defaults that were switched off and why.
#[derive(Debug, Clone)]
pub struct Libp2pPeerDiscovery {
    handle: NetworkHandle,
}

impl Libp2pPeerDiscovery {
    pub const fn new(handle: NetworkHandle) -> Self {
        Self { handle }
    }
}

impl PeerDiscoveryPort for Libp2pPeerDiscovery {
    fn announce(&self, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        let endpoints = endpoints.to_vec();

        self.handle.request(
            |reply| NetworkCommand::Announce { endpoints, reply },
            PeerDiscoveryError::Unavailable,
        )
    }

    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        self.handle.request(
            |reply| NetworkCommand::ObservePeers { reply },
            PeerDiscoveryError::Unavailable,
        )
    }

    fn redeem_join_ticket(
        &self,
        ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        // The ticket's expiry and protocol compatibility are deliberately not
        // re-checked here: that is `JoinTicket::validate`, a pure domain rule
        // the application applies first. Checking it here as well would put a
        // clock on both sides of the boundary and let the two disagree.
        let ticket = Box::new(ticket.clone());

        self.handle.request(
            |reply| NetworkCommand::RedeemTicket { ticket, reply },
            PeerDiscoveryError::TicketUnreachable,
        )
    }
}
