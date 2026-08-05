use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use shared_types::{Envelope, EnvelopeSignature, Fingerprint, PeerId};

/// The one signer and verifier behind all four crypto ports (canvas §4).
///
/// # One object, four impls, one key
///
/// `identity` and `messaging` each declare their own `EnvelopeSignerPort` and
/// their own `EnvelopeVerifierPort` — a re-export would be a cross-context
/// import and `shared_types` hosts no port traits (canvas §2.4, §4, architect
/// Note 5). The canvas then requires that the composition root wire all of them
/// to *the one underlying implementation*, so that an envelope `identity` signs
/// and a message `messaging` sends carry signatures from the same key, and so
/// that both contexts judge an inbound envelope identically.
///
/// This type is that one implementation, and the requirement is met by
/// construction rather than by the root remembering to: there is a single
/// [`SigningKey`] behind every impl, so wiring two ports to two different keys
/// is not expressible.
///
/// # It verifies too, and that is not scope creep
///
/// Verification needs no key of its own. A `PeerId` **is** an Ed25519 public
/// key (invariant 1), so the key to check against is read straight out of
/// `envelope.author` — no directory, no lookup, no state. Hosting the verifier
/// here rather than in a second type is what makes "both verifier ports reach
/// the same implementation" a fact about the object graph instead of a wiring
/// convention, and it gives the root one thing to construct instead of two.
///
/// # Invariant 4, made true by arithmetic
///
/// > *A message's author is the `PeerId` whose signature verifies on the
/// > envelope — never a payload field.*
///
/// Outbound: a draft naming an author this signer holds no key for is refused,
/// so this peer cannot emit an envelope asserting an identity it cannot back.
/// The two contexts name that refusal differently and each gets its own honest
/// error — `messaging` has
/// [`AuthorMismatch`](messaging::ports::EnvelopeSignerError::AuthorMismatch)
/// for exactly this, while `identity` has no such variant and gets
/// [`KeyUnavailable`](identity::ports::EnvelopeSignerError::KeyUnavailable),
/// which is literally true: this signer holds no key for that author. That is
/// the same split `infra-sim-net`'s `SimSigner` makes, and the two must agree
/// or a scenario would prove something about the simulator only.
///
/// Inbound: the signature is checked against `envelope.author` over
/// [`Envelope::signable_bytes`] — the layout `shared_types` pins, never a copy
/// kept here.
///
/// # A bad signature is data, never a panic and never an `Err`
///
/// Hostile and corrupted input is the normal case on an open network, so a
/// signature that does not verify — or an author whose bytes do not decode to a
/// point at all — yields
/// [`SignatureVerdict::Invalid`](identity::ports::SignatureVerdict::Invalid).
/// The error variant of each verifier port means the check could not be
/// *performed*, which cannot happen here and is therefore never returned:
/// reporting it would let a caller confuse "unknown" with "forged" (AC6).
///
/// Verification is strict ([`VerifyingKey::verify_strict`]), which rejects
/// signatures made under small-order public keys. That is deliberately stricter
/// than architect Note 2's ruling on *accepting* weak `PeerId`s: the canvas
/// accepts a weak key as an identity, and such a key's own signatures being
/// unverifiable harms only that identity — exactly the trade the note records.
/// `infra-sim-net` makes the same choice.
///
/// # The key does not leave
///
/// There is no accessor for it, no `Clone`, and [`Debug`](fmt::Debug) is
/// hand-written to print the public fingerprint and nothing else. The only way
/// to obtain one of these is
/// [`FileIdentityKeyStore::load_or_create_signer`](crate::FileIdentityKeyStore::load_or_create_signer),
/// so a signer always speaks for a real key file.
pub struct LocalEnvelopeSigner {
    signing: SigningKey,
    peer: PeerId,
}

impl LocalEnvelopeSigner {
    /// A signer speaking for `signing`.
    ///
    /// Crate-private: key material enters this crate in exactly one place — the
    /// keystore — and a signer that could be assembled anywhere would be a
    /// second one.
    pub(crate) fn new(signing: SigningKey) -> Self {
        let peer = peer_of(&signing);

        Self { signing, peer }
    }

    /// The peer this signer speaks for.
    pub const fn peer(&self) -> PeerId {
        self.peer
    }

    /// Whether `envelope`'s signature verifies under the key in its `author`
    /// field.
    ///
    /// Associated rather than a method because it uses no state: verification
    /// is about the envelope and the key it names, never about this peer.
    pub fn verifies(envelope: &Envelope) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(envelope.author.as_bytes()) else {
            return false;
        };

        let signature = Signature::from_bytes(envelope.signature.as_bytes());

        key.verify_strict(&envelope.signable_bytes(), &signature)
            .is_ok()
    }

    /// Signs the bytes of a draft this peer is entitled to sign.
    ///
    /// Takes the bytes rather than a draft because the two contexts hand over
    /// two different draft types, and the one thing they agree on is exactly
    /// this byte string — which is [`Envelope::signable_bytes`] in both cases.
    fn sign_bytes(&self, message: &[u8]) -> EnvelopeSignature {
        EnvelopeSignature::new(self.signing.sign(message).to_bytes())
    }
}

impl identity::ports::EnvelopeSignerPort for LocalEnvelopeSigner {
    fn sign(
        &self,
        unsigned: &identity::domain::UnsignedEnvelope,
    ) -> Result<EnvelopeSignature, identity::ports::EnvelopeSignerError> {
        if unsigned.author() != self.peer {
            // `identity` has no `AuthorMismatch`; "this signer holds no key for
            // that author" is what `KeyUnavailable` says, and it is true.
            return Err(identity::ports::EnvelopeSignerError::KeyUnavailable);
        }

        Ok(self.sign_bytes(&unsigned.signable_bytes()))
    }
}

impl messaging::ports::EnvelopeSignerPort for LocalEnvelopeSigner {
    fn sign(
        &self,
        unsigned: &messaging::ports::UnsignedEnvelope,
    ) -> Result<EnvelopeSignature, messaging::ports::EnvelopeSignerError> {
        if unsigned.author() != self.peer {
            return Err(messaging::ports::EnvelopeSignerError::AuthorMismatch);
        }

        Ok(self.sign_bytes(&unsigned.signable_bytes()))
    }
}

impl identity::ports::EnvelopeVerifierPort for LocalEnvelopeSigner {
    fn verify(
        &self,
        envelope: &Envelope,
    ) -> Result<identity::ports::SignatureVerdict, identity::ports::EnvelopeVerifierError> {
        Ok(if Self::verifies(envelope) {
            identity::ports::SignatureVerdict::Valid
        } else {
            identity::ports::SignatureVerdict::Invalid
        })
    }
}

impl messaging::ports::EnvelopeVerifierPort for LocalEnvelopeSigner {
    fn verify(
        &self,
        envelope: &Envelope,
    ) -> Result<messaging::ports::SignatureVerdict, messaging::ports::EnvelopeVerifierError> {
        Ok(if Self::verifies(envelope) {
            messaging::ports::SignatureVerdict::Valid
        } else {
            messaging::ports::SignatureVerdict::Invalid
        })
    }
}

impl fmt::Debug for LocalEnvelopeSigner {
    /// Hand-written so no secret byte can reach a log, a panic message, or a
    /// test failure diff — `SigningKey`'s own `Debug` prints key material, and
    /// a derived impl here would put it in every `{:?}` of anything holding a
    /// signer. The public fingerprint is the whole of what is printable about a
    /// keypair.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalEnvelopeSigner")
            .field("peer", &Fingerprint::of(&self.peer).to_string())
            .finish_non_exhaustive()
    }
}

/// Derives the public identity from a signing key.
///
/// The single derivation site in this crate, so the `PeerId` the keystore
/// reports and the one the signer speaks for cannot disagree. Infallible by
/// construction: an Ed25519 verifying key is by invariant 1 a valid [`PeerId`].
pub(crate) fn peer_of(signing: &SigningKey) -> PeerId {
    PeerId::from_public_key_bytes(signing.verifying_key().to_bytes())
        .expect("an Ed25519 verifying key is by construction a valid PeerId")
}
