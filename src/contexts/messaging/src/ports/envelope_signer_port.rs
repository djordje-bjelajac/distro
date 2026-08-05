use std::fmt;

use shared_types::{Envelope, EnvelopeSignature};

use crate::ports::UnsignedEnvelope;

/// Produces this peer's signature over an envelope it is about to send.
///
/// # Why `messaging` declares its own
///
/// `identity` has a trait of the same name, and this is deliberately not it. A
/// re-export would be a cross-context import (canvas §4, architect Note 5), and
/// `shared_types` hosts no port traits (canvas §2.4). Each context states the
/// need it has in its own terms; the composition root wires both to the one
/// underlying signer, so a message this context sends and a message `identity`
/// signs carry signatures from the same key.
///
/// This port is the outbound half of invariant 4. The verifier makes the
/// inbound half true — "the author is whoever the signature verifies for" —
/// and this makes the claim true of everything this peer emits.
///
/// # What exactly gets signed
///
/// Implementations sign [`UnsignedEnvelope::signable_bytes`] and nothing else.
/// Those bytes are [`Envelope::signable_bytes`], the layout pinned in
/// `shared_types`, so a signature made here verifies against the envelope that
/// reaches any peer whatever codec carried it (S2). The port takes a draft
/// rather than a loose `&[u8]` precisely so an adapter cannot be asked to sign
/// something that is not an envelope.
///
/// # Key material never crosses
///
/// The keypair stays inside the implementation. A draft goes in, a signature
/// comes back, and no secret byte moves in either direction — which is what
/// lets the same signer serve two contexts without either of them holding a
/// key.
pub trait EnvelopeSignerPort {
    /// Signs `unsigned.signable_bytes()` with the local peer's key.
    ///
    /// Implementations **must** reject a draft whose
    /// [`author`](UnsignedEnvelope::author) is not the peer whose key they
    /// hold, with [`EnvelopeSignerError::AuthorMismatch`]. Signing it anyway
    /// would produce an envelope asserting an identity this peer cannot back,
    /// which no verifier would accept and which nothing should ever emit.
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError>;

    /// Signs the draft and completes it into a wire-ready [`Envelope`].
    ///
    /// Provided so callers never re-derive the signing input themselves: the
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
    /// The draft names an author other than the peer this signer speaks for.
    AuthorMismatch,
}

impl fmt::Display for EnvelopeSignerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyUnavailable => f.write_str("local signing key is unavailable"),
            Self::SigningFailed => f.write_str("envelope could not be signed"),
            Self::AuthorMismatch => {
                f.write_str("the draft names an author this signer holds no key for")
            }
        }
    }
}

impl std::error::Error for EnvelopeSignerError {}
