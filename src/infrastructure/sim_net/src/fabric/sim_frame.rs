use membership::domain::Endpoint;
use messaging::domain::MessageId;
use messaging::ports::MessagePayload;
use shared_types::{Envelope, PayloadKind};

/// One thing in flight between two simulated peers.
///
/// # Why the set is exactly this
///
/// Each variant is a call the destination's *inbound* ports can take, and
/// nothing else exists. That is safeguard S3 made structural: wire data reaches
/// a context only through `InboundSessionPort` or `InboundEnvelopePort`, so a
/// frame that could not be turned into one of those calls would be a frame the
/// simulation had no honest way to deliver.
///
/// The split between session frames and message frames mirrors the ports:
/// `membership` owns how a peer is reached, `messaging` owns what is said, and
/// no frame carries both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimFrame {
    /// A remote dialled this peer; the link is up but not yet authenticated.
    ///
    /// Carries the dialer's endpoints, because a peer that redeemed *our* join
    /// ticket dials before we have ever discovered it — this is how it enters
    /// the roster at all.
    SessionOpened { endpoints: Vec<Endpoint> },
    /// The authenticated handshake completed; the moment `PeerConnected` is
    /// published.
    SessionEstablished,
    /// The link ended, for whatever reason the far side observed.
    SessionClosed,
    /// A signed envelope — direct or broadcast, told apart by its
    /// [`PayloadKind`](shared_types::PayloadKind).
    Message(Envelope),
    /// The recipient acknowledged a 1:1 message (AC11).
    ///
    /// Carries the identifier as the **sender** knows it: the recipient's
    /// `Direct(alice)` message is the sender's `Direct(bob)` message, and the
    /// acknowledgement is meaningless in the wrong conversation.
    Acknowledgement(MessageId),
    /// Evidence of life, feeding presence derivation (AC5).
    Heartbeat,
}

impl SimFrame {
    /// The compact form this frame takes in a trace.
    pub fn label(&self) -> FrameLabel {
        match self {
            Self::SessionOpened { .. } => FrameLabel::SessionOpened,
            Self::SessionEstablished => FrameLabel::SessionEstablished,
            Self::SessionClosed => FrameLabel::SessionClosed,
            Self::Message(envelope) => {
                let sequence = sequence_of(envelope);

                match envelope.kind {
                    PayloadKind::BroadcastMessage => FrameLabel::Broadcast(sequence),
                    PayloadKind::DirectMessage => FrameLabel::Direct(sequence),
                    PayloadKind::Heartbeat | PayloadKind::Unknown(_) => {
                        FrameLabel::Payload(envelope.kind.code())
                    }
                }
            }
            Self::Acknowledgement(id) => FrameLabel::Acknowledgement(id.sequence().as_u64()),
            Self::Heartbeat => FrameLabel::Heartbeat,
        }
    }

    /// Whether this frame carries a message, and so is subject to the
    /// per-message delay script, duplication, and signature corruption.
    ///
    /// Session frames deliberately are not: a script that meant "reorder these
    /// three messages" would otherwise be consumed by handshake traffic the
    /// scenario never wrote down.
    pub const fn is_message(&self) -> bool {
        matches!(self, Self::Message(_))
    }
}

/// A frame's sequence number, or `0` when the payload does not carry one.
///
/// A payload this build cannot read still deserves a trace line — it is
/// precisely the case AC14's tolerance rule and the `MalformedPayload`
/// rejection are about — so an undecodable payload renders as `0` rather than
/// making the frame unrenderable.
fn sequence_of(envelope: &Envelope) -> u64 {
    MessagePayload::decode(&envelope.payload).map_or(0, |payload| payload.sequence().as_u64())
}

/// A frame as one token in a trace line.
///
/// Copy and small on purpose: a trace holds many of these and must stay cheap
/// to clone and compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameLabel {
    SessionOpened,
    SessionEstablished,
    SessionClosed,
    /// A 1:1 message and its sequence number.
    Direct(u64),
    /// A broadcast message and its sequence number.
    Broadcast(u64),
    /// An acknowledgement for the message with this sequence number.
    Acknowledgement(u64),
    Heartbeat,
    /// A payload kind this build does not act on, by wire code (S2, AC14).
    Payload(u16),
}

impl FrameLabel {
    /// The token this label renders as in a trace.
    pub fn render(&self) -> String {
        match self {
            Self::SessionOpened => "session-opened".to_owned(),
            Self::SessionEstablished => "session-established".to_owned(),
            Self::SessionClosed => "session-closed".to_owned(),
            Self::Direct(sequence) => format!("direct#{sequence}"),
            Self::Broadcast(sequence) => format!("broadcast#{sequence}"),
            Self::Acknowledgement(sequence) => format!("ack#{sequence}"),
            Self::Heartbeat => "heartbeat".to_owned(),
            Self::Payload(code) => format!("payload({code})"),
        }
    }
}

/// Why the fabric did not hand a frame to its destination.
///
/// Every cause is a *stated* outcome that appears in the trace. A simulated
/// network that silently swallowed frames would make an unexplained missing
/// message indistinguishable from a bug in the code under test — which is the
/// one thing this harness exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropCause {
    /// The destination's process is not running.
    DestinationOffline,
    /// The destination left the simulation.
    DestinationUnknown,
    /// A partition came down between enqueue and delivery, so the frame was
    /// in flight across a split that no longer carries traffic.
    Partitioned,
    /// The individual link was severed while the frame was in flight.
    LinkSevered,
}

impl DropCause {
    /// The token this cause renders as in a trace.
    pub const fn token(self) -> &'static str {
        match self {
            Self::DestinationOffline => "offline",
            Self::DestinationUnknown => "unknown-peer",
            Self::Partitioned => "partitioned",
            Self::LinkSevered => "link-severed",
        }
    }
}
