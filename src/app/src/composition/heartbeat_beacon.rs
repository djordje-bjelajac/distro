use std::fmt;
use std::sync::Arc;

use messaging::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, MessageTransportError, MessageTransportPort,
    UnsignedEnvelope,
};
use shared_types::{PayloadKind, PeerId, ProtocolVersion};

/// The liveness probe this peer emits on the presence tick.
///
/// # Why the root emits it and nothing else does
///
/// `PayloadKind::Heartbeat` exists in `shared_types` and `infra-net-libp2p`
/// deliberately never produces one: a liveness probe is not a transport
/// concern, and `messaging` has no port for sending anything but a text
/// message — its `SendMessagePort` composes a `MessageBody` into a
/// conversation, which a heartbeat is not and must not become. Nobody would
/// want a keep-alive in their broadcast history.
///
/// So the beacon is assembled where every other cross-cutting wiring is: at the
/// root, from parts that already exist. It drafts an envelope through
/// `messaging`'s own `UnsignedEnvelope`, has it signed by the same signer every
/// message uses, and releases it on the broadcast topic through the same
/// transport port. No new type reaches the wire and no context learns anything.
///
/// # What the other side does with it
///
/// Two things, and only the first is `messaging`'s:
///
/// * `InboundEnvelopePort::accept_envelope` answers
///   `InboundVerdict::Ignored(Heartbeat)` — a well-formed envelope carrying a
///   kind it does not act on, tolerated and counted (S2, AC14).
/// * The root reports the sender to `InboundSessionPort::peer_heartbeat`, which
///   is what re-arms presence (invariant 7, AC5). That happens for *every*
///   `EnvelopeReceived`, not only heartbeats — any traffic at all is evidence
///   of life, and a peer holding a conversation needs no extra probe.
///
/// # The payload is empty on purpose
///
/// A timestamp inside it would be the author's claim about their own clock,
/// which no rule in either context may read (see `Millis`); presence is derived
/// from *this* peer's arrival instant. An empty payload is the honest encoding
/// of "I am here", and it keeps a heartbeat to the envelope header plus a
/// signature.
///
/// # It is signed like everything else
///
/// An unsigned heartbeat would be a free way to assert another peer's presence,
/// which invariant 4 and invariant 7 both forbid. The signature costs one
/// Ed25519 operation every ten seconds.
pub struct HeartbeatBeacon {
    local: PeerId,
    protocol: ProtocolVersion,
    signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
    transport: Arc<dyn MessageTransportPort + Send + Sync>,
}

impl HeartbeatBeacon {
    /// A beacon speaking for `local`.
    pub const fn new(
        local: PeerId,
        protocol: ProtocolVersion,
        signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
    ) -> Self {
        Self {
            local,
            protocol,
            signer,
            transport,
        }
    }

    /// Signs and releases one heartbeat on the broadcast topic.
    pub fn emit(&self) -> Result<(), HeartbeatError> {
        let draft = UnsignedEnvelope::draft(
            self.local,
            self.protocol,
            PayloadKind::Heartbeat,
            Vec::new(),
        );

        let envelope = self.signer.seal(draft)?;

        self.transport.publish_broadcast(&envelope)?;
        Ok(())
    }
}

impl fmt::Debug for HeartbeatBeacon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeartbeatBeacon")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// Why a heartbeat did not go out.
///
/// Never fatal: a missed heartbeat costs this peer some freshness in other
/// peers' rosters, and the next tick tries again. It is counted so a pane can
/// show that a peer which looks isolated is failing to speak rather than
/// failing to hear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatError {
    /// The envelope could not be signed.
    Signer(EnvelopeSignerError),
    /// The broadcast topic would not take it.
    Transport(MessageTransportError),
}

impl From<EnvelopeSignerError> for HeartbeatError {
    fn from(error: EnvelopeSignerError) -> Self {
        Self::Signer(error)
    }
}

impl From<MessageTransportError> for HeartbeatError {
    fn from(error: MessageTransportError) -> Self {
        Self::Transport(error)
    }
}

impl fmt::Display for HeartbeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signer(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for HeartbeatError {}
