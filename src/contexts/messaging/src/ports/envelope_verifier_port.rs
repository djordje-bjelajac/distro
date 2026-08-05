use std::fmt;

use shared_types::Envelope;

use crate::ports::SignatureVerdict;

/// Checks an inbound envelope's signature against the
/// [`PeerId`](shared_types::PeerId) in its `author` field.
///
/// # Why `messaging` declares its own
///
/// `identity` has a trait of the same name, and this is deliberately not it: a
/// re-export would be a cross-context import, and `shared_types` hosts no port
/// traits (canvas §4, architect Note 5). This trait states only what this
/// context needs — verify an envelope against its author — and the composition
/// root wires it to the same underlying implementation `identity` uses.
///
/// # What it makes true
///
/// Invariant 4: a message's author is the `PeerId` whose signature verifies on
/// the envelope, never a payload field. `AcceptInboundMessage` (OP-7) calls
/// this *before* handing anything to
/// [`Conversation::accept_remote`](crate::domain::Conversation::accept_remote),
/// which trusts the author it is given. The bytes checked are
/// [`Envelope::signable_bytes`], the same layout the signer signed.
///
/// # Failure is data, not a panic
///
/// A signature that does not verify yields [`SignatureVerdict::Invalid`].
/// Hostile input is the normal case on an open network, so no implementation
/// may panic, unwrap, or abort on a malformed signature.
/// [`EnvelopeVerifierError`] is reserved for the different situation where the
/// check could not be *performed*; a caller must never read that as "valid",
/// and the distinction exists so diagnostics can tell a forged envelope apart
/// from a broken verifier (AC6).
pub trait EnvelopeVerifierPort {
    /// Verifies `envelope`'s signature against its author.
    fn verify(&self, envelope: &Envelope) -> Result<SignatureVerdict, EnvelopeVerifierError>;
}

/// Typed failure of an [`EnvelopeVerifierPort`] operation — the check could not
/// be carried out. Never a statement about the signature itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeVerifierError {
    /// The verifier could not run at all; the envelope's authenticity remains
    /// unknown and it must not be treated as verified.
    VerifierUnavailable,
}

impl fmt::Display for EnvelopeVerifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifierUnavailable => {
                f.write_str("signature verifier is unavailable; authenticity is unknown")
            }
        }
    }
}

impl std::error::Error for EnvelopeVerifierError {}
