//! Inbound and outbound port traits of the `membership` context (canvas §4).
//!
//! Every trait is `Port`-suffixed, takes `&self`, and is object-safe, so a
//! composition root can hold one behind `Arc<dyn …>` and tests can substitute
//! an in-memory fake. Ports depend on `domain` and `shared_types` only — never
//! on `application` or any adapter type, and no `libp2p`, socket, or async
//! machinery appears in a signature.
//!
//! # The two directions
//!
//! **Outbound** (driven) ports are what this context calls: [`PeerDiscoveryPort`],
//! [`PeerTransportPort`], [`PeerCachePort`], [`ClockPort`], [`EventPublisherPort`].
//!
//! **Inbound** (driving) ports are how this context is called:
//! [`JoinNetworkPort`] for the deliberate membership decisions a user or a
//! startup step makes, [`InboundSessionPort`] for everything the network
//! reports back, and [`MembershipQueryPort`] for reads. Their arguments are
//! `domain` and `shared_types` types rather than the application's command
//! DTOs — a port may not name an application type, so the imperative commands
//! live in `application/commands/` and the services implementing these traits
//! build them from these arguments. The dependency keeps pointing inward.
//!
//! [`ClockPort`] is deliberately **this context's own** trait rather than a
//! shared one: `shared_types` is a data-contract crate that hosts no port
//! (canvas §2.4), and importing another context's clock would be a
//! cross-context import. Each context declares the time it needs in its own
//! terms — here, [`Millis`](crate::domain::Millis) — and the composition root
//! wires them all to one implementation.

mod bootstrap_attempt;
mod bootstrap_rung;
mod cached_peer;
mod clock_port;
#[cfg(test)]
mod clock_port_test;
mod discovered_peer;
mod discovery_outcome;
mod event_publisher_port;
#[cfg(test)]
mod event_publisher_port_test;
mod inbound_session_port;
mod join_diagnostic;
#[cfg(test)]
mod join_diagnostic_test;
mod join_network_port;
mod join_outcome;
mod known_peer_view;
mod leave_outcome;
mod membership_command_error;
mod membership_query_port;
mod peer_cache_port;
#[cfg(test)]
mod peer_cache_port_test;
mod peer_discovery_port;
#[cfg(test)]
mod peer_discovery_port_test;
mod peer_transport_port;
#[cfg(test)]
mod peer_transport_port_test;
#[cfg(test)]
pub(crate) mod port_fakes;
mod rung_failure;

pub use bootstrap_attempt::BootstrapAttempt;
pub use bootstrap_rung::BootstrapRung;
pub use cached_peer::CachedPeer;
pub use clock_port::ClockPort;
pub use discovered_peer::DiscoveredPeer;
pub use discovery_outcome::DiscoveryOutcome;
pub use event_publisher_port::{EventPublisherError, EventPublisherPort};
pub use inbound_session_port::InboundSessionPort;
pub use join_diagnostic::JoinDiagnostic;
pub use join_network_port::JoinNetworkPort;
pub use join_outcome::JoinOutcome;
pub use known_peer_view::KnownPeerView;
pub use leave_outcome::LeaveOutcome;
pub use membership_command_error::MembershipCommandError;
pub use membership_query_port::MembershipQueryPort;
pub use peer_cache_port::{PeerCacheError, PeerCachePort};
pub use peer_discovery_port::{PeerDiscoveryError, PeerDiscoveryPort};
pub use peer_transport_port::{PeerTransportError, PeerTransportPort};
pub use rung_failure::RungFailure;
