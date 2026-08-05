use std::fmt;

use shared_types::{Envelope, PeerId};

use crate::domain::DeliveryFailure;

/// How a signed envelope leaves this peer (canvas §4).
///
/// # Addressed by `PeerId`, and nothing else
///
/// There is no endpoint, address, multiaddress, socket, or reachability class
/// in this signature — nor anywhere else in this crate. That is a hard
/// architectural rule, not an omission: `membership` owns how a peer is
/// reached, and if this trait learned about addresses the two contexts would be
/// coupled through it (canvas §4). This context knows a peer by identity and
/// asks for the message to get there; whether that happens over a direct link
/// or through a third peer acting as relay (AC12) is entirely below this line.
///
/// # Envelopes by reference
///
/// Both methods borrow. D10's bounded retry cycle re-sends *the same* envelope,
/// and re-signing or cloning it per attempt would be waste at best and a second
/// signature over the same message at worst.
pub trait MessageTransportPort {
    /// Sends a 1:1 message to one peer (D4).
    ///
    /// Returning `Ok` means the transport accepted it, not that anyone read
    /// it; delivery state is tracked separately (AC11).
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError>;

    /// Releases a message to the network-wide broadcast channel (D3).
    ///
    /// There is no recipient and no acknowledgement: gossip reaches whoever is
    /// online and subscribed (AC10), which is why broadcast messages are
    /// `Published` and never `Delivered`.
    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError>;
}

/// Typed failure of a [`MessageTransportPort`] operation.
///
/// Every variant maps onto a [`DeliveryFailure`] via
/// [`as_delivery_failure`](Self::as_delivery_failure) — that mapping is the
/// whole reason the set is shaped this way. AC11 makes silent loss a non-state,
/// so a send that fails must arrive at something a user can read and act on,
/// and a variant with no honest delivery meaning would break that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageTransportError {
    /// The transport is not running, so nothing was attempted.
    Unavailable,
    /// No path to the peer existed; they are likely offline.
    PeerUnreachable,
    /// A direct path failed and no peer was available to relay (S7, AC12).
    NoRelayAvailable,
    /// The session died before the envelope was handed over (D10).
    SessionClosed,
    /// The envelope went out and was never acknowledged within the bounded
    /// retry cycle (D10).
    NotAcknowledged,
}

impl MessageTransportError {
    /// What this failure means for the message the user is looking at.
    pub const fn as_delivery_failure(&self) -> DeliveryFailure {
        match self {
            Self::Unavailable => DeliveryFailure::TransportUnavailable,
            Self::PeerUnreachable => DeliveryFailure::PeerUnreachable,
            Self::NoRelayAvailable => DeliveryFailure::NoRelayAvailable,
            Self::SessionClosed => DeliveryFailure::SessionClosed,
            Self::NotAcknowledged => DeliveryFailure::RetriesExhausted,
        }
    }
}

impl fmt::Display for MessageTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the message transport is not available"),
            Self::PeerUnreachable => f.write_str("the recipient could not be reached"),
            Self::NoRelayAvailable => {
                f.write_str("no peer was available to relay to the recipient")
            }
            Self::SessionClosed => f.write_str("the session closed before the message was sent"),
            Self::NotAcknowledged => f.write_str("the message was never acknowledged"),
        }
    }
}

impl std::error::Error for MessageTransportError {}
