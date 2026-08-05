use ed25519_dalek::{Signature, VerifyingKey};
use shared_types::Envelope;

/// The one verifier behind both contexts' `EnvelopeVerifierPort`s (canvas §4).
///
/// # Invariant 4, made true by arithmetic
///
/// > *A message's author is the `PeerId` whose signature verifies on the
/// > envelope — never a payload field.*
///
/// A `PeerId` **is** an Ed25519 public key (invariant 1), so this needs no key
/// directory, no trust store, and no lookup of any kind: the key to check
/// against is read straight out of `envelope.author`, and the bytes checked are
/// [`Envelope::signable_bytes`] — the layout `shared_types` pins and the signer
/// signed. Nothing stateful, so one instance serves every peer.
///
/// # A bad signature is data, never a panic
///
/// Hostile and corrupted input is the normal case on an open network, so a
/// signature that does not verify — or one whose author bytes do not decode to
/// a point at all — yields `Invalid`. The error variant of each port means the
/// check could not be *performed*, which cannot happen here and is therefore
/// never returned: reporting it would let a caller confuse "unknown" with
/// "forged" (AC6).
///
/// Verification is strict (`verify_strict`), which rejects signatures made
/// under small-order public keys. That is stricter than architect Note 2's
/// ruling on *accepting* weak `PeerId`s and deliberately so: the canvas accepts
/// a weak key as an identity, and a weak key's own signatures being
/// unverifiable harms only that identity — exactly the trade the note records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimVerifier;

impl SimVerifier {
    /// Whether `envelope`'s signature verifies under the key in its `author`
    /// field.
    pub fn verifies(envelope: &Envelope) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(envelope.author.as_bytes()) else {
            return false;
        };

        let signature = Signature::from_bytes(envelope.signature.as_bytes());
        key.verify_strict(&envelope.signable_bytes(), &signature)
            .is_ok()
    }
}

impl identity::ports::EnvelopeVerifierPort for SimVerifier {
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

impl messaging::ports::EnvelopeVerifierPort for SimVerifier {
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
