use std::sync::Arc;

use shared_types::{EnvelopeSignature, PeerId};

use crate::crypto::SimKeypair;

/// The one signer behind both contexts' `EnvelopeSignerPort`s (canvas §4).
///
/// # Why one type implements two traits
///
/// `identity` and `messaging` each declare their own signer port — a re-export
/// would be a cross-context import and `shared_types` hosts no port traits
/// (canvas §2.4, §4, architect Note 5). The canvas then says the composition
/// root wires both to *the one underlying signer*, so that a message
/// `messaging` sends and an envelope `identity` signs carry signatures from the
/// same key. This type is that one signer, and wiring the two ports to two
/// different objects is not expressible: there is a single [`SimKeypair`]
/// behind both impls.
///
/// # The author check is not a formality
///
/// Both impls refuse a draft naming an author this signer holds no key for.
/// Signing it anyway would produce an envelope asserting an identity this peer
/// cannot back — no verifier would accept it, and the sender would have no way
/// to tell why its messages vanished. The two ports name that refusal
/// differently, so each gets its own honest error:
/// `messaging` has [`AuthorMismatch`](messaging::ports::EnvelopeSignerError::AuthorMismatch)
/// for exactly this, while `identity` has no such variant and gets
/// [`KeyUnavailable`](identity::ports::EnvelopeSignerError::KeyUnavailable) —
/// which is literally true: this signer holds no key for that author.
pub struct SimSigner {
    keypair: Arc<SimKeypair>,
}

impl SimSigner {
    /// A signer speaking for the peer that owns `keypair`.
    pub const fn new(keypair: Arc<SimKeypair>) -> Self {
        Self { keypair }
    }

    /// The peer this signer speaks for.
    pub fn peer(&self) -> PeerId {
        self.keypair.peer()
    }
}

impl identity::ports::EnvelopeSignerPort for SimSigner {
    fn sign(
        &self,
        unsigned: &identity::domain::UnsignedEnvelope,
    ) -> Result<EnvelopeSignature, identity::ports::EnvelopeSignerError> {
        if unsigned.author() != self.keypair.peer() {
            return Err(identity::ports::EnvelopeSignerError::KeyUnavailable);
        }

        Ok(self.keypair.sign_bytes(&unsigned.signable_bytes()))
    }
}

impl messaging::ports::EnvelopeSignerPort for SimSigner {
    fn sign(
        &self,
        unsigned: &messaging::ports::UnsignedEnvelope,
    ) -> Result<EnvelopeSignature, messaging::ports::EnvelopeSignerError> {
        if unsigned.author() != self.keypair.peer() {
            return Err(messaging::ports::EnvelopeSignerError::AuthorMismatch);
        }

        Ok(self.keypair.sign_bytes(&unsigned.signable_bytes()))
    }
}
