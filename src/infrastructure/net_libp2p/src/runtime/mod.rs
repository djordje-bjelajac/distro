//! Starting the network, and the synchronous seam between the ports and the
//! swarm.
//!
//! The ports this crate implements are synchronous by design — no `tokio`,
//! socket, or async machinery may appear in a port signature (canvas §4) — and
//! a `libp2p` swarm is a state machine that must be polled from exactly one
//! place. [`NetworkRuntime`] owns the runtime and the one driver task;
//! [`NetworkHandle`] is what the adapters hold; [`NetworkEvents`] is what the
//! composition root drains. The full contract is documented on
//! [`NetworkRuntime`], and OP-12 is expected to honour all six points of it.

mod network_config;
mod network_events;
mod network_handle;
mod network_identity;
mod network_runtime;
#[cfg(test)]
mod network_runtime_test;

pub use network_config::NetworkConfig;
pub use network_events::NetworkEvents;
pub use network_handle::NetworkHandle;
pub use network_identity::{NetworkIdentity, NetworkIdentityError};
pub use network_runtime::{NetworkRuntime, NetworkStartError};
