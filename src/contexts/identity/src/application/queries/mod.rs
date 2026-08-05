//! Query handlers: the paths that only read `identity` state.
//!
//! No handler here writes — not to the trust record store, not to the local
//! identity. Asking about a peer that was never verified or blocked reports
//! the trust-on-first-use default without creating a record for it, so
//! rendering a roster cannot silently populate the store.
//!
//! The read models these return ([`LocalIdentitySummary`](crate::ports::LocalIdentitySummary),
//! [`PeerTrustState`](crate::ports::PeerTrustState)) belong to the inbound
//! `IdentityQueryPort` contract and therefore live in `ports/`.

mod get_local_identity;
#[cfg(test)]
mod get_local_identity_test;
mod get_peer_trust_state;
#[cfg(test)]
mod get_peer_trust_state_test;
mod identity_query_service;
mod list_blocked_peers;
#[cfg(test)]
mod list_blocked_peers_test;

pub use get_local_identity::{GetLocalIdentity, GetLocalIdentityHandler};
pub use get_peer_trust_state::{GetPeerTrustState, GetPeerTrustStateHandler};
pub use identity_query_service::IdentityQueryService;
pub use list_blocked_peers::{ListBlockedPeers, ListBlockedPeersHandler};
