//! Query handlers: the paths that only read `membership` state.
//!
//! No handler here writes. That is not a convention this module asks to be
//! trusted on — [`Presence`](crate::domain::Presence) is *derived* at read
//! time from the age of the roster's evidence (invariant 7), so a query that
//! wrote would be turning a derivation into a fact someone set, and this
//! module's tests assert the roster is byte-identical after any number of
//! reads.
//!
//! The read models these return ([`KnownPeerView`](crate::ports::KnownPeerView),
//! [`NetworkStatus`](crate::domain::NetworkStatus)) belong to the inbound
//! `MembershipQueryPort` contract and therefore live in `ports/` and `domain/`
//! rather than here.

mod get_network_status;
#[cfg(test)]
mod get_network_status_test;
mod list_known_peers;
#[cfg(test)]
mod list_known_peers_test;
mod list_online_peers;
#[cfg(test)]
mod list_online_peers_test;
mod membership_query_service;

pub use get_network_status::{GetNetworkStatus, GetNetworkStatusHandler};
pub use list_known_peers::{ListKnownPeers, ListKnownPeersHandler};
pub use list_online_peers::{ListOnlinePeers, ListOnlinePeersHandler};
pub use membership_query_service::MembershipQueryService;
