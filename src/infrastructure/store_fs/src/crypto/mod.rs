//! The real Ed25519 seam this peer signs and verifies through.
//!
//! # Why it lives in this crate
//!
//! Signing needs the secret key, the secret key lives in a file, and that file
//! is [`FileIdentityKeyStore`](crate::FileIdentityKeyStore)'s. Putting the
//! signer anywhere else would mean handing key material across a boundary to
//! get there — which is precisely what `IdentityKeyStorePort` is shaped to
//! prevent (it returns a `PeerId` and nothing else). So the signer is built
//! *here*, from the key, and the key never moves.
//!
//! # One implementation, four ports
//!
//! [`LocalEnvelopeSigner`] implements both contexts' signer ports and both
//! contexts' verifier ports over one key. Canvas §4 requires the composition
//! root to wire all four to one underlying implementation; making them impls on
//! one object means the root cannot do otherwise. `infra-sim-net` splits the
//! same behaviour across `SimSigner` and `SimVerifier` because the harness
//! needs to hand a peer's signer and the network's verifier to different
//! places; the behaviour they implement is identical, and it has to stay that
//! way, since every multi-peer claim is verified against the simulator (S5).

mod local_envelope_signer;
#[cfg(test)]
mod local_envelope_signer_test;

pub use local_envelope_signer::LocalEnvelopeSigner;
pub(crate) use local_envelope_signer::peer_of;
