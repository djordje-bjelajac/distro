//! Aggregates, value objects, events, and typed errors of the `identity`
//! context (canvas §2.1).
//!
//! Nothing here depends on `ports` or `adapters`: the domain names what is
//! true about identity and trust, never how key material is stored or how
//! bytes are signed. [`LocalIdentity`] therefore holds no key material at
//! all — it drafts an [`UnsignedEnvelope`], and only `EnvelopeSignerPort` can
//! turn that into a signed [`Envelope`](shared_types::Envelope).

pub mod events;

mod display_name;
#[cfg(test)]
mod display_name_test;
mod local_identity;
#[cfg(test)]
mod local_identity_test;
mod trust_record;
#[cfg(test)]
mod trust_record_test;
mod unsigned_envelope;
#[cfg(test)]
mod unsigned_envelope_test;
mod verification_state;

pub use display_name::{DisplayName, DisplayNameError};
pub use local_identity::LocalIdentity;
pub use trust_record::{TrustRecord, TrustRecordError};
pub use unsigned_envelope::UnsignedEnvelope;
pub use verification_state::VerificationState;
