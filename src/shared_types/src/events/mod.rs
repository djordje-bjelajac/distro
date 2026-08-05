//! Cross-context peer lifecycle events (canvas §2.2†): published by
//! `membership`, consumed by other contexts. Payload is [`PeerId`] only, so
//! no context learns another's internals — `messaging` must never see an
//! endpoint, session, or address.
//!
//! [`PeerId`]: crate::PeerId

mod peer_connected;
#[cfg(test)]
mod peer_connected_test;
mod peer_disconnected;
#[cfg(test)]
mod peer_disconnected_test;

pub use peer_connected::PeerConnected;
pub use peer_disconnected::PeerDisconnected;
