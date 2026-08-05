use shared_types::{PeerConnected, PeerDisconnected};

use crate::domain::events::{NetworkJoined, NetworkLeft, PeerDiscovered, PeerPresenceExpired};

/// Everything the `membership` context publishes (canvas §2.2).
///
/// The union exists so `EventPublisherPort` can be object-safe with one method
/// — a trait with a method per event could not be held behind `dyn` and would
/// grow a breaking change every time an event is added. Adding a variant here
/// makes every exhaustive publisher fail to compile, which is the intended
/// pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipEvent {
    NetworkJoined(NetworkJoined),
    NetworkLeft(NetworkLeft),
    PeerDiscovered(PeerDiscovered),
    PeerPresenceExpired(PeerPresenceExpired),
    /// Cross-context (`shared_types`): consumed by other contexts.
    PeerConnected(PeerConnected),
    /// Cross-context (`shared_types`): consumed by other contexts.
    PeerDisconnected(PeerDisconnected),
}

impl MembershipEvent {
    /// Whether this event leaves the context.
    ///
    /// Only the two `shared_types` events do. An adapter that bridges contexts
    /// uses this to avoid leaking membership internals — endpoints, sessions,
    /// presence — to consumers that must never see them (canvas §4).
    pub const fn is_cross_context(&self) -> bool {
        matches!(self, Self::PeerConnected(_) | Self::PeerDisconnected(_))
    }
}

impl From<NetworkJoined> for MembershipEvent {
    fn from(event: NetworkJoined) -> Self {
        Self::NetworkJoined(event)
    }
}

impl From<NetworkLeft> for MembershipEvent {
    fn from(event: NetworkLeft) -> Self {
        Self::NetworkLeft(event)
    }
}

impl From<PeerDiscovered> for MembershipEvent {
    fn from(event: PeerDiscovered) -> Self {
        Self::PeerDiscovered(event)
    }
}

impl From<PeerPresenceExpired> for MembershipEvent {
    fn from(event: PeerPresenceExpired) -> Self {
        Self::PeerPresenceExpired(event)
    }
}

impl From<PeerConnected> for MembershipEvent {
    fn from(event: PeerConnected) -> Self {
        Self::PeerConnected(event)
    }
}

impl From<PeerDisconnected> for MembershipEvent {
    fn from(event: PeerDisconnected) -> Self {
        Self::PeerDisconnected(event)
    }
}
