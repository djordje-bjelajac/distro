//! Where libp2p's vocabulary becomes this network's, and stops.
//!
//! Canvas D2's containment rule in one module: no `libp2p` type crosses the
//! adapter boundary, so every translation between a libp2p identity or address
//! and a `shared_types`/`membership` one happens here. Both mappings are
//! total in the sense that matters on an open network — every input a stranger
//! can choose produces a value or a typed refusal, never a panic.

mod endpoint_mapping;
#[cfg(test)]
mod endpoint_mapping_test;
mod peer_id_mapping;
#[cfg(test)]
mod peer_id_mapping_test;

pub use endpoint_mapping::{EndpointMapping, EndpointMappingError};
pub use peer_id_mapping::{PeerIdMapping, PeerIdMappingError};
