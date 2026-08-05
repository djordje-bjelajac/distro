use std::fmt;

use crate::domain::events::MembershipEvent;

/// Where this context's events go (canvas §4).
///
/// One method over the [`MembershipEvent`] union rather than a method per
/// event: that keeps the trait object-safe, and it keeps adding an event from
/// being a breaking change to every implementation.
///
/// This port is how `PeerConnected` and `PeerDisconnected` reach other contexts
/// without any context importing another (canvas §4). The domain returns those
/// events in a [`SessionOutcome`](crate::domain::SessionOutcome); the
/// application hands them here; an adapter delivers them. `messaging` learns a
/// `PeerId` became reachable and nothing else — no endpoint, no session, no
/// presence.
pub trait EventPublisherPort {
    /// Publishes one event.
    ///
    /// Order is significant across calls: a `PeerDisconnected` that overtook
    /// its `PeerConnected` would leave a consumer believing a dead peer is
    /// live.
    fn publish(&self, event: MembershipEvent) -> Result<(), EventPublisherError>;
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
