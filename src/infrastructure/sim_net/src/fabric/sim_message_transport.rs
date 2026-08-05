use std::sync::Arc;

use messaging::ports::{MessageTransportError, MessageTransportPort};
use shared_types::{Envelope, PeerId};

use crate::fabric::SimFabric;

/// One peer's `MessageTransportPort` over the shared fabric.
///
/// # Addressed by `PeerId`, and nothing else
///
/// There is no endpoint, address, or reachability argument in this type — the
/// same hard rule the port states. This adapter knows a peer by identity and
/// asks the fabric to get the envelope there; whether that happens over a
/// direct link or through a third peer acting as relay (AC12) is decided below
/// this line, and `messaging` never learns which.
///
/// # A refusal is a delivery state, not a lost message
///
/// Every error variant this returns maps onto a `DeliveryFailure` the user can
/// read (AC11). The mapping is the honest one — an unreachable peer, a missing
/// relay, and a closed session are three different sentences — because the
/// whole point of AC11 is that silent loss is not a state.
pub struct SimMessageTransport {
    peer: PeerId,
    fabric: Arc<SimFabric>,
}

impl SimMessageTransport {
    /// A message transport speaking for `peer`.
    pub const fn new(peer: PeerId, fabric: Arc<SimFabric>) -> Self {
        Self { peer, fabric }
    }
}

impl MessageTransportPort for SimMessageTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        self.fabric.send_direct(self.peer, to, envelope)
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        self.fabric.publish_broadcast(self.peer, envelope)
    }
}
