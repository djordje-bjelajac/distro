//! The three port implementations, one per file.
//!
//! Each is a thin translation: build a command, block for the answer, return a
//! domain-shaped result. They hold a [`NetworkHandle`](crate::runtime::NetworkHandle)
//! and nothing else, so the swarm's lifetime is the runtime's and not theirs —
//! a root may hand these to three contexts behind `Arc<dyn …Port + Send + Sync>`
//! and drop them in any order.

mod libp2p_message_transport;
mod libp2p_peer_discovery;
mod libp2p_peer_transport;

pub use libp2p_message_transport::Libp2pMessageTransport;
pub use libp2p_peer_discovery::Libp2pPeerDiscovery;
pub use libp2p_peer_transport::Libp2pPeerTransport;
