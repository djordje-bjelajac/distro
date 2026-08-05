//! The swarm and the single task that drives it.
//!
//! Everything in here is crate-private except the event vocabulary: a
//! composition root talks to this layer through
//! [`NetworkRuntime`](crate::runtime::NetworkRuntime) and the three port
//! adapters, never by holding a `Swarm`, a `ConnectionId`, or a `Multiaddr`.
//! That is canvas D2's containment rule enforced by visibility rather than by
//! convention.

pub(crate) mod direct_message_codec;
#[cfg(test)]
mod direct_message_codec_test;
pub(crate) mod distro_behaviour;
#[cfg(test)]
mod distro_behaviour_test;
pub(crate) mod link_registry;
#[cfg(test)]
mod link_registry_test;
pub(crate) mod network_command;
pub(crate) mod network_driver;
mod network_event;

pub use network_event::{DirectMessageFailure, NetworkEvent};
