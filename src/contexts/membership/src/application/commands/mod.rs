//! Command handlers: the paths that change `membership` state.
//!
//! Each command is an imperative DTO naming a use case, each handler is named
//! by intent, and every handler returns what its change produced — the
//! past-tense events, or the [`SessionOutcome`](crate::domain::SessionOutcome)
//! whose consequences the caller must carry out. Nothing here returns a read
//! model; that is [`queries`](crate::application::queries).
//!
//! The commands live here rather than in `ports/` because a port may depend on
//! `domain` and `shared_types` only. The inbound ports therefore speak in
//! domain types, and [`JoinNetworkService`] / [`InboundSessionService`] build
//! these DTOs from them.
//!
//! # Two services, because there are two kinds of caller
//!
//! [`JoinNetworkService`] carries the decisions a person or a startup step
//! makes — join, leave, connect to that peer. [`InboundSessionService`] carries
//! what the network reports — a peer announced itself, a remote dialled in, a
//! handshake finished, a link died, nobody has spoken in a while. The failure
//! modes are different (a user can be told to try again; a wire event cannot),
//! and so is the trust placed in the argument.
//!
//! # Events are published outside the roster lock
//!
//! Every handler takes the lock, performs one pure domain transition, releases
//! it, and only then publishes. The roster returns its consequences rather than
//! enacting them precisely so this is possible, and it is what lets a port call
//! back into the query side while a command is still running.

mod close_session;
#[cfg(test)]
mod close_session_test;
mod establish_session;
#[cfg(test)]
mod establish_session_test;
mod expire_presence;
#[cfg(test)]
mod expire_presence_test;
mod forget_known_peers;
#[cfg(test)]
mod forget_known_peers_test;
mod inbound_session_service;
mod join_network;
mod join_network_service;
#[cfg(test)]
mod join_network_test;
mod leave_network;
#[cfg(test)]
mod leave_network_test;
mod open_session;
#[cfg(test)]
mod open_session_test;
mod record_discovered_peer;
#[cfg(test)]
mod record_discovered_peer_test;
mod record_peer_heartbeat;
#[cfg(test)]
mod record_peer_heartbeat_test;
mod session_close_cause;

pub use close_session::{CloseSession, CloseSessionHandler};
pub use establish_session::{EstablishSession, EstablishSessionHandler};
pub use expire_presence::{ExpirePresence, ExpirePresenceHandler};
pub use forget_known_peers::{ForgetKnownPeers, ForgetKnownPeersHandler};
pub use inbound_session_service::InboundSessionService;
pub use join_network::{JoinNetwork, JoinNetworkHandler};
pub use join_network_service::JoinNetworkService;
pub use leave_network::{LeaveNetwork, LeaveNetworkHandler};
pub use open_session::{OpenSession, OpenSessionHandler};
pub use record_discovered_peer::{RecordDiscoveredPeer, RecordDiscoveredPeerHandler};
pub use record_peer_heartbeat::{RecordPeerHeartbeat, RecordPeerHeartbeatHandler};
pub use session_close_cause::SessionCloseCause;
