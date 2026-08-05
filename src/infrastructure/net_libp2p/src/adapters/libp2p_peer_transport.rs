use membership::domain::Endpoint;
use membership::ports::{PeerTransportError, PeerTransportPort};
use shared_types::PeerId;

use crate::runtime::NetworkHandle;
use crate::swarm::network_command::NetworkCommand;

/// `membership`'s `PeerTransportPort` over the real swarm.
///
/// Deliberately thin: it turns each call into a command and blocks for the
/// answer. Everything that decides anything — which endpoint answered, which
/// link the collapse rule kept — lives in the driver, because that is the only
/// place that can see the connections.
///
/// # `close_session` closes by peer, and that is now sufficient
///
/// OP-6 flagged that this method cannot name one of two links during a
/// simultaneous connect. The resolution is below this line: the driver applies
/// the domain's own `SessionCollapse::resolve` the moment a second connection
/// appears and closes the superseded one itself, so the application never holds
/// two sessions for one peer and this call has exactly one meaning — *end this
/// peer's session*. See
/// [`LinkRegistry`](crate::swarm::link_registry::LinkRegistry) for the full
/// argument.
///
/// # What is not here
///
/// No method that sends bytes to a peer. `messaging` addresses peers through
/// its own `MessageTransportPort`, and a transport trait that grew a "send"
/// here would couple the two contexts through it (canvas §4).
#[derive(Debug, Clone)]
pub struct Libp2pPeerTransport {
    handle: NetworkHandle,
}

impl Libp2pPeerTransport {
    pub const fn new(handle: NetworkHandle) -> Self {
        Self { handle }
    }
}

impl PeerTransportPort for Libp2pPeerTransport {
    fn listen(&self) -> Result<Vec<Endpoint>, PeerTransportError> {
        self.handle.request(
            |reply| NetworkCommand::Listen { reply },
            PeerTransportError::ListenFailed,
        )
    }

    fn dial(&self, peer: PeerId, endpoints: &[Endpoint]) -> Result<Endpoint, PeerTransportError> {
        let endpoints = endpoints.to_vec();

        self.handle.request(
            |reply| NetworkCommand::Dial {
                peer,
                endpoints,
                reply,
            },
            // A dial that produced no answer inside the request timeout is,
            // from the caller's point of view, exactly "no endpoint answered" —
            // S7's known limit, which the UI must be able to state rather than
            // retry forever.
            PeerTransportError::NoReachableEndpoint,
        )
    }

    fn close_session(&self, peer: PeerId) -> Result<(), PeerTransportError> {
        self.handle.request(
            |reply| NetworkCommand::CloseSession { peer, reply },
            PeerTransportError::Unavailable,
        )
    }
}
