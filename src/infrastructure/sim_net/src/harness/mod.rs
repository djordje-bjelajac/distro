//! N peers in one process, over one deterministic network.
//!
//! [`SimNetwork`] is what a scenario programs against: it owns the clock, the
//! fabric, the trace, and every peer's three assembled contexts. [`SimPeer`] is
//! one peer, wired the way a composition root wires one. [`DurablePeerState`]
//! is the line between what survives a restart and what does not — which is
//! D12 and AC16 expressed as a type rather than as a convention.

mod durable_peer_state;
mod sim_network;
mod sim_network_builder;
#[cfg(test)]
mod sim_network_test;
mod sim_peer;
mod sim_settings;

pub use durable_peer_state::DurablePeerState;
pub use sim_network::SimNetwork;
pub use sim_network_builder::SimNetworkBuilder;
pub use sim_peer::SimPeer;
pub use sim_settings::SimSettings;
