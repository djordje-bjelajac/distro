//! Domain events of the `membership` context (canvas §2.2), all past tense.
//!
//! Four of them are **context-internal**: they name things only this context
//! observes, and their payloads are free to mention this context's own value
//! objects. The other two — `PeerConnected` and `PeerDisconnected` — are the
//! **cross-context** contracts published from `shared_types`, carrying a
//! `PeerId` and nothing else, so no other context ever learns what an
//! `Endpoint`, a `Session`, or a `Presence` is.
//!
//! [`MembershipEvent`] unions both groups, which is what lets
//! `EventPublisherPort` stay object-safe with a single method.

mod membership_event;
#[cfg(test)]
mod membership_event_test;
mod network_joined;
mod network_left;
mod peer_discovered;
mod peer_presence_expired;

pub use membership_event::MembershipEvent;
pub use network_joined::NetworkJoined;
pub use network_left::NetworkLeft;
pub use peer_discovered::PeerDiscovered;
pub use peer_presence_expired::PeerPresenceExpired;
