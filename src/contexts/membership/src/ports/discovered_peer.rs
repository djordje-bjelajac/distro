use shared_types::PeerId;

use crate::domain::Endpoint;

/// A peer as a discovery mechanism reports it: an identity and where it claims
/// to be reachable.
///
/// Not a domain entity — it becomes one only once
/// [`PeerRoster::record_discovery`](crate::domain::PeerRoster::record_discovery)
/// accepts it, which is where the local peer is filtered out (invariant 2) and
/// the endpoint cap applies. Keeping this a plain port-level struct means an
/// adapter can report what it saw without being able to mutate the roster.
///
/// Nothing here is trustworthy: an announcement is a claim by whoever made it.
/// The identity is only proven at the session handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer: PeerId,
    pub endpoints: Vec<Endpoint>,
}
