use std::sync::Arc;

use membership::domain::Endpoint;
use membership::ports::{PeerTransportError, PeerTransportPort};
use shared_types::PeerId;

use crate::fabric::SimFabric;

/// One peer's `PeerTransportPort` over the shared fabric.
///
/// Deliberately thin: it holds the peer it speaks for and delegates. All the
/// routing lives in [`SimFabric`], because a message can only reach a peer a
/// dial could reach — one network, three ports.
///
/// # What is not here
///
/// No method that sends bytes to a peer. `messaging` addresses peers through
/// its own `MessageTransportPort`, and a transport trait that grew a "send" here
/// would couple the two contexts through it (canvas §4). The split is what keeps
/// `messaging` from ever learning what an [`Endpoint`] is.
pub struct SimPeerTransport {
    peer: PeerId,
    fabric: Arc<SimFabric>,
}

impl SimPeerTransport {
    /// A transport speaking for `peer`.
    pub const fn new(peer: PeerId, fabric: Arc<SimFabric>) -> Self {
        Self { peer, fabric }
    }
}

impl PeerTransportPort for SimPeerTransport {
    fn listen(&self) -> Result<Vec<Endpoint>, PeerTransportError> {
        self.fabric.listen(self.peer)
    }

    fn dial(&self, peer: PeerId, endpoints: &[Endpoint]) -> Result<Endpoint, PeerTransportError> {
        self.fabric.dial(self.peer, peer, endpoints)
    }

    fn close_session(&self, peer: PeerId) -> Result<(), PeerTransportError> {
        self.fabric.close(self.peer, peer)
    }
}
