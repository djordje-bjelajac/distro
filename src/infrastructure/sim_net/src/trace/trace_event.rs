use std::collections::BTreeMap;
use std::fmt::Write as _;

use membership::domain::events::MembershipEvent;
use messaging::domain::events::MessagingEvent;
use messaging::domain::{ConversationId, MessageId};
use shared_types::PeerId;

use crate::fabric::{DropCause, FrameLabel};

/// One thing that happened in a simulated run, in the vocabulary a scenario
/// reasons in.
///
/// # Why the fabric and the contexts share one stream
///
/// The canvas requires that the same seed and the same script produce a
/// byte-identical trace. A trace of only the contexts' events would prove the
/// application layers are deterministic while leaving the network free to
/// deliver in a different order each run; a trace of only the network would
/// prove the reverse. Interleaving them in one ordered stream is what makes the
/// claim cover the whole simulation.
///
/// Every variant is derived from data the simulation already produced —
/// nothing is invented for the trace, so a rendered line can always be traced
/// back to a port call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    /// The harness started, stopped, or restarted a peer.
    Lifecycle { peer: PeerId, change: PeerLifecycle },
    /// The fabric handed a frame to its destination.
    FrameDelivered {
        from: PeerId,
        to: PeerId,
        frame: FrameLabel,
    },
    /// The fabric had a frame for a destination that could not take it.
    FrameDropped {
        from: PeerId,
        to: PeerId,
        frame: FrameLabel,
        cause: DropCause,
    },
    /// The destination took the frame and its inbound port refused it.
    ///
    /// Distinct from a drop: the bytes arrived and the peer decided. A blocked
    /// author, a session already open, an acknowledgement for a message that
    /// had already failed — all land here, and all are legitimate outcomes
    /// worth seeing in a trace.
    FrameRefused {
        from: PeerId,
        to: PeerId,
        frame: FrameLabel,
        reason: String,
    },
    /// A port call the harness made on a peer's behalf was refused.
    ///
    /// The harness drives four things no frame carries — the identity
    /// assumption, the join, the two clock-driven sweeps — and fans
    /// `membership`'s peer lifecycle into `messaging`. A refusal from any of
    /// them belongs in the trace rather than being swallowed: a scenario whose
    /// gap never closed deserves to see that the sweep itself was refused.
    PortRefused {
        peer: PeerId,
        operation: &'static str,
        reason: String,
    },
    /// A peer's `membership` context published an event.
    Membership {
        peer: PeerId,
        event: MembershipEvent,
    },
    /// A peer's `messaging` context published an event.
    Messaging { peer: PeerId, event: MessagingEvent },
}

/// What the harness did to a peer's process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerLifecycle {
    /// The peer entered the simulation, or came back online.
    Started,
    /// The process died abruptly. Nothing was announced — peers learn by
    /// presence expiry (AC5).
    Stopped,
    /// The process was replaced: identity, peer cache, trust records, and the
    /// outbound sequence counter survived; conversations did not (D7, D12).
    Restarted,
}

impl TraceEvent {
    /// Renders this event as one line, resolving peers to the labels a
    /// scenario named them with.
    ///
    /// Deterministic by construction: every field is either an integer, a
    /// fixed token, or a label looked up in `labels`. No pointer, no hash
    /// order, no clock.
    pub fn render(&self, labels: &BTreeMap<PeerId, String>) -> String {
        let name = |peer: &PeerId| label_of(labels, peer);

        match self {
            Self::Lifecycle { peer, change } => {
                format!("{} {}", name(peer), lifecycle_token(*change))
            }
            Self::FrameDelivered { from, to, frame } => {
                format!("{} -> {} {}", name(from), name(to), frame.render())
            }
            Self::FrameDropped {
                from,
                to,
                frame,
                cause,
            } => format!(
                "{} -x {} {} dropped({})",
                name(from),
                name(to),
                frame.render(),
                cause.token()
            ),
            Self::FrameRefused {
                from,
                to,
                frame,
                reason,
            } => format!(
                "{} -! {} {} refused({reason})",
                name(from),
                name(to),
                frame.render()
            ),
            Self::PortRefused {
                peer,
                operation,
                reason,
            } => format!("{} refused {operation}: {reason}", name(peer)),
            Self::Membership { peer, event } => {
                format!("{} {}", name(peer), render_membership(event, labels))
            }
            Self::Messaging { peer, event } => {
                format!("{} {}", name(peer), render_messaging(event, labels))
            }
        }
    }
}

const fn lifecycle_token(change: PeerLifecycle) -> &'static str {
    match change {
        PeerLifecycle::Started => "started",
        PeerLifecycle::Stopped => "stopped",
        PeerLifecycle::Restarted => "restarted",
    }
}

/// The label a scenario gave `peer`, or a short digest of its key when the
/// trace has never been told about it.
///
/// The fallback is deterministic too: an unlabelled peer must not make a trace
/// stop matching between runs.
pub(crate) fn label_of(labels: &BTreeMap<PeerId, String>, peer: &PeerId) -> String {
    labels.get(peer).cloned().unwrap_or_else(|| {
        let mut rendered = String::from("peer:");
        for byte in &peer.as_bytes()[..4] {
            let _ = write!(rendered, "{byte:02x}");
        }
        rendered
    })
}

fn render_conversation(conversation: ConversationId, labels: &BTreeMap<PeerId, String>) -> String {
    match conversation {
        ConversationId::Broadcast => "broadcast".to_owned(),
        ConversationId::Direct(peer) => format!("direct({})", label_of(labels, &peer)),
    }
}

fn render_message_id(id: MessageId, labels: &BTreeMap<PeerId, String>) -> String {
    format!(
        "{}/{}#{}",
        render_conversation(id.conversation(), labels),
        label_of(labels, &id.author()),
        id.sequence().as_u64()
    )
}

fn render_membership(event: &MembershipEvent, labels: &BTreeMap<PeerId, String>) -> String {
    match event {
        MembershipEvent::NetworkJoined(joined) => format!(
            "network-joined peers={} at={}",
            joined.connected_peers,
            joined.at.as_millis()
        ),
        MembershipEvent::NetworkLeft(left) => {
            format!("network-left at={}", left.at.as_millis())
        }
        MembershipEvent::PeerDiscovered(discovered) => format!(
            "peer-discovered {} at={}",
            label_of(labels, &discovered.peer),
            discovered.at.as_millis()
        ),
        MembershipEvent::PeerPresenceExpired(expired) => format!(
            "peer-presence-expired {} last-evidence={} at={}",
            label_of(labels, &expired.peer),
            expired.last_evidence_at.as_millis(),
            expired.at.as_millis()
        ),
        MembershipEvent::PeerConnected(connected) => {
            format!("peer-connected {}", label_of(labels, &connected.peer))
        }
        MembershipEvent::PeerDisconnected(disconnected) => {
            format!("peer-disconnected {}", label_of(labels, &disconnected.peer))
        }
    }
}

fn render_messaging(event: &MessagingEvent, labels: &BTreeMap<PeerId, String>) -> String {
    match event {
        MessagingEvent::MessageSent(sent) => format!(
            "message-sent {} claimed-at={}",
            render_message_id(sent.id, labels),
            sent.claimed_sent_at.as_millis()
        ),
        MessagingEvent::MessageReceived(received) => format!(
            "message-received {} claimed-at={}",
            render_message_id(received.id, labels),
            received.claimed_sent_at.as_millis()
        ),
        MessagingEvent::MessageRejected(rejected) => format!(
            "message-rejected {} author={} sequence={} reason={:?}",
            render_conversation(rejected.conversation, labels),
            label_of(labels, &rejected.claimed_author),
            rejected.sequence.map_or_else(
                || "none".to_owned(),
                |sequence| sequence.as_u64().to_string()
            ),
            rejected.reason
        ),
        MessagingEvent::MessageDuplicateIgnored(duplicate) => format!(
            "message-duplicate-ignored {}",
            render_message_id(duplicate.id, labels)
        ),
        MessagingEvent::MessageGapClosed(closed) => format!(
            "message-gap-closed {} author={} range={}..={} cause={:?}",
            render_conversation(closed.conversation, labels),
            label_of(labels, &closed.author),
            closed.from.as_u64(),
            closed.to.as_u64(),
            closed.cause
        ),
        MessagingEvent::MessageDeliveryStateChanged(changed) => format!(
            "message-delivery {} {:?} -> {:?}",
            render_message_id(changed.id, labels),
            changed.from,
            changed.to
        ),
    }
}
