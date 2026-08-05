//! Inbound and outbound port traits of the `identity` context (canvas §4).
//!
//! Every trait is `Port`-suffixed, takes `&self`, and is object-safe, so a
//! composition root can hold one behind `Arc<dyn …>` and tests can substitute
//! an in-memory fake. Ports depend on `domain` and `shared_types` only —
//! never on `application` or any adapter type. The contract types the inbound
//! traits speak in (outcomes, read models, typed errors) therefore live here
//! too; the imperative command DTOs that name each use case live with their
//! handlers in `application/commands/`.
//!
//! Outbound (driven): [`IdentityKeyStorePort`], [`TrustRecordStorePort`],
//! [`EnvelopeSignerPort`], [`EnvelopeVerifierPort`].
//! Inbound (driving): [`IdentityCommandPort`], [`IdentityQueryPort`], which
//! `application` implements and the composition root calls.
//!
//! The two crypto ports are deliberately separate from the key store: the
//! store yields a public [`PeerId`](shared_types::PeerId) and the signer
//! consumes drafts, so no operation in this context can return secret key
//! bytes to a caller.

mod envelope_signer_port;
#[cfg(test)]
mod envelope_signer_port_test;
mod envelope_verifier_port;
#[cfg(test)]
mod envelope_verifier_port_test;
mod identity_command_port;
mod identity_key_store_port;
#[cfg(test)]
mod identity_key_store_port_test;
mod identity_query_port;
mod local_identity_assumption;
#[cfg(test)]
mod local_identity_assumption_test;
mod local_identity_summary;
mod peer_trust_command_error;
#[cfg(test)]
mod peer_trust_command_error_test;
mod peer_trust_state;
#[cfg(test)]
pub(crate) mod port_fakes;
mod set_display_name_error;
#[cfg(test)]
mod set_display_name_error_test;
mod signature_verdict;
mod trust_record_store_port;
#[cfg(test)]
mod trust_record_store_port_test;

pub use envelope_signer_port::{EnvelopeSignerError, EnvelopeSignerPort};
pub use envelope_verifier_port::{EnvelopeVerifierError, EnvelopeVerifierPort};
pub use identity_command_port::IdentityCommandPort;
pub use identity_key_store_port::{IdentityKeyStoreError, IdentityKeyStorePort};
pub use identity_query_port::IdentityQueryPort;
pub use local_identity_assumption::LocalIdentityAssumption;
pub use local_identity_summary::LocalIdentitySummary;
pub use peer_trust_command_error::PeerTrustCommandError;
pub use peer_trust_state::PeerTrustState;
pub use set_display_name_error::SetDisplayNameError;
pub use signature_verdict::SignatureVerdict;
pub use trust_record_store_port::{TrustRecordStoreError, TrustRecordStorePort};
