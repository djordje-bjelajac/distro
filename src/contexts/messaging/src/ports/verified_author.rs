use shared_types::{Envelope, PeerId};

use crate::ports::{EnvelopeVerifierError, EnvelopeVerifierPort, SignatureVerdict};

/// A `PeerId` that a signature actually verified for (invariant 4).
///
/// # What this type is for
///
/// [`Conversation::accept_remote`](crate::domain::Conversation::accept_remote)
/// **trusts** the author it is handed, and says so in a doc comment. A doc
/// comment is the weakest safeguard there is: it is checked by whoever reads
/// it. This type turns that precondition into something the compiler checks,
/// because the inbound pipeline's final step takes a `VerifiedAuthor` and there
/// is no way to obtain one except [`attest`](Self::attest) — which runs the
/// verifier over a real envelope and reads the author out of *that envelope*
/// rather than from an argument a caller could supply.
///
/// So a future handler that tries to reach the conversation with an author it
/// merely parsed out of a payload does not fail a review; it fails to compile.
///
/// # Neither `Clone` nor `Copy`, deliberately
///
/// One attestation, one use. A verified author that could be duplicated and
/// stashed would invite exactly the mistake this type exists to prevent: using
/// yesterday's verdict to admit today's envelope.
///
/// The field is private to this module, so nothing else in this crate can
/// construct one either — the guarantee holds inside the context, not only
/// across its boundary.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedAuthor(PeerId);

impl VerifiedAuthor {
    /// Runs `verifier` over `envelope` and, if the signature verifies, mints
    /// the attestation naming the envelope's author.
    ///
    /// The three outcomes are kept apart because they mean three different
    /// things (AC6):
    ///
    /// * `Ok(Some(_))` — the signature verified; this peer *is* the author.
    /// * `Ok(None)` — the signature did not verify. Expected input on an open
    ///   network, never an error, and the content must not reach a read model
    ///   (invariant 10).
    /// * `Err(_)` — the check could not be *performed*. Authenticity is
    ///   unknown, which is not the same as invalid and must never be read as
    ///   valid.
    pub fn attest(
        verifier: &dyn EnvelopeVerifierPort,
        envelope: &Envelope,
    ) -> Result<Option<Self>, EnvelopeVerifierError> {
        Ok(match verifier.verify(envelope)? {
            SignatureVerdict::Valid => Some(Self(envelope.author)),
            SignatureVerdict::Invalid => None,
        })
    }

    /// The attested peer, without consuming the attestation.
    pub const fn peer(&self) -> PeerId {
        self.0
    }

    /// Consumes the attestation and yields the peer it established.
    ///
    /// The consuming form is the one the pipeline uses: handing the author to
    /// the conversation is the moment the attestation is spent.
    pub const fn into_peer(self) -> PeerId {
        self.0
    }
}
