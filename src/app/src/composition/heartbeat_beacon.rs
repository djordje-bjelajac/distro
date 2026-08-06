use std::fmt;
use std::sync::Arc;

use messaging::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, MessageTransportPort, UnsignedEnvelope,
};
use shared_types::{PayloadKind, PeerId, ProtocolVersion};

use crate::composition::HeartbeatLedger;

/// The liveness probe this peer emits on the presence tick, to each peer it
/// holds a session with.
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
/// message uses, and hands it to the same transport port. No new type reaches
/// the wire and no context learns anything.
///
/// # Why it is sent to sessions rather than published (canvas `0010` D7)
///
/// It used to go out on the broadcast topic, and that made liveness depend on
/// gossip-mesh formation. The failure was observed: two instances showing
/// `connected (2 peers)` above a roster in which every row read `offline`,
/// because the sessions were alive and no envelope ever arrived over them.
///
/// Moving to direct sessions **loses nothing**, and that is the decisive
/// argument rather than a hope. Evidence of life is credited to the *carrier*
/// of a frame, never its author (invariant 2), the carrier of any gossip
/// message is a peer this instance holds a connection with, and the roster
/// holds a session for essentially every libp2p connection. So the peers a
/// broadcast heartbeat could ever have produced evidence about were already a
/// subset of the peers holding sessions.
///
/// What it gains is a **round trip**. A direct message is acknowledged, so one
/// heartbeat yields evidence in both directions: the recipient gets
/// `EnvelopeReceived { from }` and reports it as evidence about us, and we get
/// `DirectMessageDelivered { peer }` and report it as evidence about them
/// (D6). A healthy session therefore produces mutual evidence every
/// `HEARTBEAT_INTERVAL`, and `Linked(Offline)` appears only when something is
/// genuinely broken.
///
/// There is exactly one mechanism. Nothing here publishes.
///
/// # One signature per round, not one per peer
///
/// A round drafts and signs **once** and sends that one envelope to every
/// linked peer. The envelope has no recipient field and no nonce, so a per-peer
/// signature would be the same bytes signed repeatedly — the same Ed25519
/// operation done *n* times for an identical result. Sending one signed
/// envelope to *n* peers is what a broadcast was, minus the mesh.
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
/// Ed25519 operation every ten seconds — one per *round*, not one per peer.
///
/// # The signature is remembered, and not in the delivery index (S6)
///
/// Because a heartbeat now travels as a direct message, the transport answers
/// for it exactly as it answers for a real one. Those answers name a signature,
/// and the message correlation must not be the thing that recognises them: a
/// heartbeat has no `MessageId`, so an unreachable peer would raise *"a message
/// to X was not delivered"* on every tick. The signature therefore goes into a
/// [`HeartbeatLedger`] of its own, which the event router consults *before* the
/// delivery index. See that type for why the two are kept apart rather than
/// merged.
pub struct HeartbeatBeacon {
    local: PeerId,
    protocol: ProtocolVersion,
    signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
    transport: Arc<dyn MessageTransportPort + Send + Sync>,
    heartbeats: Arc<HeartbeatLedger>,
}

impl HeartbeatBeacon {
    /// A beacon speaking for `local`, recording what it releases in
    /// `heartbeats`.
    pub const fn new(
        local: PeerId,
        protocol: ProtocolVersion,
        signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
        heartbeats: Arc<HeartbeatLedger>,
    ) -> Self {
        Self {
            local,
            protocol,
            signer,
            transport,
            heartbeats,
        }
    }

    /// Signs one heartbeat and sends it to each of `linked`.
    ///
    /// `linked` is the set of peers holding an **established** session, decided
    /// by the caller from `membership`'s own classification — the beacon takes
    /// no roster and asks no query, because the root already holds both the
    /// query port and the tick that drives this.
    ///
    /// An empty `linked` is the ordinary state of an instance nobody has
    /// dialled yet. It sends nothing, signs nothing, and is **not** an error:
    /// there is no failure in having nobody to speak to, and reporting one
    /// would put a fault on screen for a fresh install behaving exactly as
    /// designed.
    pub fn emit(&self, linked: &[PeerId]) -> Result<HeartbeatRound, HeartbeatError> {
        if linked.is_empty() {
            return Ok(HeartbeatRound::default());
        }

        let draft = UnsignedEnvelope::draft(
            self.local,
            self.protocol,
            PayloadKind::Heartbeat,
            Vec::new(),
        );

        let envelope = self.signer.seal(draft)?;

        // Recorded before the first send, not after: an acknowledgement can
        // arrive on the driver's thread while this one is still walking the
        // rest of the list, and a ledger written afterwards would let that
        // first answer be read as a message's.
        self.heartbeats.record(envelope.signature);

        let mut round = HeartbeatRound::default();

        for peer in linked {
            // A refused send fails one peer, not the round. The others are
            // still worth reaching, and which of them the transport would not
            // take is a number rather than a decision.
            match self.transport.send_direct(*peer, &envelope) {
                Ok(()) => round.sent += 1,
                Err(_) => round.refused += 1,
            }
        }

        Ok(round)
    }
}

impl fmt::Debug for HeartbeatBeacon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeartbeatBeacon")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// What one round of heartbeats did.
///
/// Two numbers rather than a pass/fail, because a round is *n* independent
/// sends and they do not share a fate: a peer whose link has just dropped costs
/// one refusal while every other peer is reached normally.
///
/// Both are zero for a round with nobody to send to, which is how an isolated
/// instance is distinguished from one that is failing to speak. Neither is a
/// claim about any peer being alive — an accepted send means the transport took
/// the envelope, and only the acknowledgement that may follow is evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeartbeatRound {
    /// Heartbeats the transport accepted for sending.
    pub sent: u64,
    /// Heartbeats the transport would not take.
    pub refused: u64,
}

/// Why a whole round of heartbeats never left.
///
/// One variant, because exactly one thing can fail a round rather than a peer:
/// the envelope is drafted and signed once, so a signer that refuses leaves
/// nothing to send to anybody. A transport refusal is per peer and is counted
/// in [`HeartbeatRound::refused`] instead — folding it in here would report the
/// loss of one link as the loss of the round.
///
/// Never fatal either way: a missed heartbeat costs this peer some freshness in
/// other peers' rosters, and the next tick tries again. It is counted so a pane
/// can show that a peer which looks isolated is failing to speak rather than
/// failing to hear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatError {
    /// The envelope could not be signed, so there was nothing to send.
    Signer(EnvelopeSignerError),
}

impl From<EnvelopeSignerError> for HeartbeatError {
    fn from(error: EnvelopeSignerError) -> Self {
        Self::Signer(error)
    }
}

impl fmt::Display for HeartbeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signer(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for HeartbeatError {}
