use std::fmt;

use shared_types::{Envelope, EnvelopeSignature};

use crate::domain::UnsignedEnvelope;

/// Produces the local peer's signature over an envelope it authored.
///
/// # What exactly gets signed
///
/// Implementations sign [`UnsignedEnvelope::signable_bytes`] and nothing else.
/// Those bytes are [`Envelope::signable_bytes`] — the layout pinned in
/// `shared_types` — so a signature made here verifies against the envelope
/// that reaches any peer, whatever codec carried it. The port takes the draft
/// rather than a loose `&[u8]` precisely so an adapter cannot sign something
/// that is not an envelope of this peer's own making.
///
/// The keypair stays inside the implementation, exactly as with
/// [`IdentityKeyStorePort`](crate::ports::IdentityKeyStorePort): the domain
/// hands over a draft and gets a signature back, and no secret byte moves in
/// either direction.
pub trait EnvelopeSignerPort {
    /// Signs `unsigned.signable_bytes()` with the local peer's key.
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError>;

    /// Signs the draft and completes it into a wire-ready [`Envelope`].
    ///
    /// Provided so callers never re-derive the signing input themselves; the
    /// only way to get a signed envelope out of a draft is through the
    /// signature this port produced for that same draft.
    fn seal(&self, unsigned: UnsignedEnvelope) -> Result<Envelope, EnvelopeSignerError> {
        let signature = self.sign(&unsigned)?;
        Ok(unsigned.into_signed(signature))
    }
}

/// Typed failure of an [`EnvelopeSignerPort`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeSignerError {
    /// The signing key is not available (key store not yet loaded, or the
    /// backing device is gone).
    KeyUnavailable,
    /// The key was available but the signing operation itself failed.
    SigningFailed,
}

impl fmt::Display for EnvelopeSignerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyUnavailable => f.write_str("local signing key is unavailable"),
            Self::SigningFailed => f.write_str("envelope could not be signed"),
        }
    }
}

impl std::error::Error for EnvelopeSignerError {}
