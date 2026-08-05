use messaging::ports::{MessageTransportError, MessageTransportPort};
use shared_types::{Envelope, PeerId};

use crate::runtime::NetworkHandle;
use crate::swarm::network_command::NetworkCommand;

/// `messaging`'s `MessageTransportPort` over the real swarm.
///
/// # Addressed by `PeerId`, and nothing else
///
/// There is no endpoint, address, or reachability argument in this type — the
/// same hard rule the port states. This adapter knows a peer by identity and
/// asks the swarm to get the envelope there; whether that happens over a direct
/// QUIC link or through a third peer's relayed circuit (AC12) is decided below
/// this line, and `messaging` never learns which.
///
/// # Where the encoding happens
///
/// On the calling thread, before the command is sent. Two reasons: the driver
/// stays free of `Envelope` (it carries frames, not messages), and an envelope
/// that cannot be encoded is refused immediately instead of failing invisibly
/// inside an async task.
///
/// # A refusal is a delivery state, not a lost message
///
/// Every error returned here maps onto a `DeliveryFailure` a user can read
/// (AC11). What this call *cannot* answer is whether the recipient took the
/// message in — that arrives later as
/// [`NetworkEvent::DirectMessageDelivered`](crate::swarm::NetworkEvent::DirectMessageDelivered)
/// or `DirectMessageFailed`, correlated by the envelope's signature. The port
/// says so explicitly: `Ok` means the transport accepted it, not that anyone
/// read it.
#[derive(Debug, Clone)]
pub struct Libp2pMessageTransport {
    handle: NetworkHandle,
}

impl Libp2pMessageTransport {
    pub const fn new(handle: NetworkHandle) -> Self {
        Self { handle }
    }
}

impl MessageTransportPort for Libp2pMessageTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        let frame = self
            .handle
            .codec()
            .encode(envelope)
            // An envelope this build cannot write out is not something the
            // network can fix, and there is no path it could take.
            .map_err(|_| MessageTransportError::Unavailable)?;
        let signature = envelope.signature;

        self.handle.request(
            |reply| NetworkCommand::SendDirect {
                to,
                signature,
                frame,
                reply,
            },
            MessageTransportError::Unavailable,
        )
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        let frame = self
            .handle
            .codec()
            .encode(envelope)
            .map_err(|_| MessageTransportError::Unavailable)?;

        self.handle.request(
            |reply| NetworkCommand::PublishBroadcast { frame, reply },
            MessageTransportError::Unavailable,
        )
    }
}
