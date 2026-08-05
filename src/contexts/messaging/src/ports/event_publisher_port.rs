use std::fmt;

use crate::domain::events::MessagingEvent;

/// Where this context's events go (canvas §4).
///
/// One method over the [`MessagingEvent`] union rather than a method per event:
/// that keeps the trait object-safe, and it keeps adding an event from being a
/// breaking change to every implementation.
///
/// Every event this context publishes is internal to it — the user interface
/// and the local diagnostics counters are the consumers. Nothing here crosses
/// into another context; traffic in that direction runs the other way, with
/// `PeerConnected` / `PeerDisconnected` arriving from `membership` through
/// `shared_types` carrying a `PeerId` and nothing else.
pub trait EventPublisherPort {
    /// Publishes one event.
    ///
    /// Order is significant across calls: a `MessageReceived` that overtook the
    /// one before it would show a conversation out of order, which is the one
    /// thing the sequencing rules exist to prevent (AC8).
    fn publish(&self, event: MessagingEvent) -> Result<(), EventPublisherError>;
}

/// Typed failure of an [`EventPublisherPort`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPublisherError {
    /// The publisher cannot accept events.
    Unavailable,
}

impl fmt::Display for EventPublisherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the event publisher is not available"),
        }
    }
}

impl std::error::Error for EventPublisherError {}
