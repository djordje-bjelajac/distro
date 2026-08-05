//! The real Ed25519 seam every simulated peer signs and verifies through.
//!
//! # Why not a digest fake
//!
//! Each context crate carries its own keyed-digest fake for unit tests, which
//! is right for asserting that *some* signature was produced over *those*
//! bytes. It is not enough for this harness. Safeguard S5 makes sim-net the
//! required vehicle for every multi-peer behaviour claim, and AC6 is one of
//! them: "every displayed message is signature-verified against the author's
//! `PeerId`; invalid or unsigned envelopes are rejected before the read model".
//! A forgery must be caught here for the reason a real verifier catches it, or
//! the claim is about the fake rather than about the system.
//!
//! # The composition-root shape, enforced by construction
//!
//! Canvas §4 requires that both contexts' signer ports be wired to one
//! underlying signer, and both verifier ports to one underlying verifier —
//! neither context importing the other. [`SimSigner`] and [`SimVerifier`] each
//! implement both traits over one keypair, so wiring the two ports to two
//! different keys is not expressible in this crate.

mod sim_key_store;
mod sim_keypair;
#[cfg(test)]
mod sim_keypair_test;
mod sim_signer;
#[cfg(test)]
mod sim_signer_test;
mod sim_verifier;
#[cfg(test)]
mod sim_verifier_test;

pub use sim_key_store::SimKeyStore;
pub use sim_keypair::SimKeypair;
pub use sim_signer::SimSigner;
pub use sim_verifier::SimVerifier;
