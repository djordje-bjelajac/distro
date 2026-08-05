//! The deterministic in-process network: one routing core and the three port
//! adapters over it.
//!
//! # One fabric, three ports
//!
//! `PeerTransportPort`, `PeerDiscoveryPort`, and `MessageTransportPort` belong
//! to two different contexts and must stay as narrow as their traits — but they
//! are three views of one network. A message can only reach a peer a dial could
//! reach, and a peer can only be dialled at an address discovery published.
//! [`SimFabric`] is that one network; the three adapters hold a `PeerId` and
//! delegate, so no context gains a concept its port does not name.
//!
//! # Nothing happens without being asked
//!
//! There is no thread, no timer, and no socket here. A frame handed to a
//! transport is queued with a due instant; it becomes deliverable when the
//! virtual clock reaches that instant, and is delivered when the harness pumps.
//! Both steps are explicit, which is what makes AC13's determinism a property
//! rather than a hope.

mod link_policy;
mod queued_frame;
mod sim_fabric;
#[cfg(test)]
mod sim_fabric_test;
mod sim_frame;
mod sim_message_transport;
mod sim_peer_discovery;
mod sim_peer_transport;

pub use link_policy::{DialFault, LinkPolicy};
pub use queued_frame::QueuedFrame;
pub use sim_fabric::SimFabric;
pub use sim_frame::{DropCause, FrameLabel, SimFrame};
pub use sim_message_transport::SimMessageTransport;
pub use sim_peer_discovery::SimPeerDiscovery;
pub use sim_peer_transport::SimPeerTransport;
