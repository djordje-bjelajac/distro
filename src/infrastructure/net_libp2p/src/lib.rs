//! `infra-net-libp2p`: the real network a peer joins (canvas D2, OP-10).
//!
//! # What this crate is
//!
//! The one place `libp2p`, `tokio`, and `ciborium` exist in this workspace. It
//! implements three ports belonging to two contexts —
//! [`PeerTransportPort`](membership::ports::PeerTransportPort) and
//! [`PeerDiscoveryPort`](membership::ports::PeerDiscoveryPort) from
//! `membership`, [`MessageTransportPort`](messaging::ports::MessageTransportPort)
//! from `messaging` — over a single swarm, because they are three views of one
//! network: a message can only reach a peer a dial could reach, and a peer can
//! only be dialled at an address discovery published. `infra-sim-net` models
//! the same three ports over a deterministic in-process fabric, and the two
//! must agree behaviourally: the simulator is what every multi-peer claim is
//! verified against (S5), and this crate is what actually carries the bytes.
//!
//! # Containment (D2)
//!
//! No `libp2p`, `tokio`, or `ciborium` type appears in anything this crate
//! hands out. Identities and addresses are translated in [`mapping`] and stop
//! there; the async runtime is owned by [`runtime::NetworkRuntime`] and never
//! named by a caller; the swarm itself is crate-private. A context crate that
//! tried to reach past a port would find nothing to reach.
//!
//! # Serverless integrity (S1)
//!
//! There is no bootstrap list, no default relay, no rendezvous point, no STUN
//! server, and no telemetry endpoint anywhere in this crate — not as a
//! constant, not as a default, not behind a feature flag. Every libp2p default
//! that would have contacted a host somebody else operates is switched off
//! explicitly, and each one is named with its reason on
//! [`DistroBehaviour`](swarm::NetworkEvent) — see `swarm/distro_behaviour.rs`.
//! The relay **server** is always on: on this network the peers *are* the
//! relays (AC4).
//!
//! # Where to start
//!
//! [`runtime::NetworkRuntime::start`] builds everything and documents the
//! threading contract a composition root must honour. It hands out the three
//! port adapters in [`adapters`] and the event queue in
//! [`runtime::NetworkEvents`].

pub mod adapters;
pub mod codec;
pub mod limits;
pub mod mapping;
pub mod runtime;
pub mod swarm;
pub mod ticket;

#[cfg(test)]
mod required_network;
#[cfg(test)]
mod required_network_test;
#[cfg(test)]
mod serverless_integrity_test;
#[cfg(test)]
mod test_peers;

pub use adapters::{Libp2pMessageTransport, Libp2pPeerDiscovery, Libp2pPeerTransport};
pub use codec::{CodecDiagnostics, EnvelopeCodec, EnvelopeCodecError};
pub use limits::ResourceLimits;
pub use runtime::{
    NetworkConfig, NetworkEvents, NetworkHandle, NetworkIdentity, NetworkIdentityError,
    NetworkRuntime, NetworkStartError,
};
pub use swarm::{DirectMessageFailure, NetworkEvent, Reachability};
pub use ticket::{JoinTicketCodec, JoinTicketCodecError};
