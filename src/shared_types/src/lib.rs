//! Published cross-context contracts: `PeerId`, `ProtocolVersion`, `Envelope`,
//! `PayloadKind`, and peer lifecycle events.
//!
//! This crate depends on nothing internal and carries no codec, transport, or
//! async machinery: encoding lives in adapters, signing and verification live
//! behind identity-context ports.

mod compatibility;
#[cfg(test)]
mod compatibility_test;
mod envelope;
mod envelope_signature;
#[cfg(test)]
mod envelope_signature_test;
#[cfg(test)]
mod envelope_test;
mod events;
mod fingerprint;
#[cfg(test)]
mod fingerprint_test;
mod payload_kind;
#[cfg(test)]
mod payload_kind_test;
mod peer_id;
#[cfg(test)]
mod peer_id_test;
mod protocol_version;
#[cfg(test)]
mod protocol_version_test;

pub use compatibility::Compatibility;
pub use envelope::Envelope;
pub use envelope_signature::EnvelopeSignature;
pub use events::{PeerConnected, PeerDisconnected};
pub use fingerprint::Fingerprint;
pub use payload_kind::PayloadKind;
pub use peer_id::{PeerId, PeerIdError};
pub use protocol_version::ProtocolVersion;
