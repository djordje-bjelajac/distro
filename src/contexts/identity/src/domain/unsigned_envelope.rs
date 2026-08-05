use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

/// An [`Envelope`] drafted by the local peer but not yet signed.
///
/// This type exists so that signing can be a port operation without the
/// domain ever touching key material: [`LocalIdentity`](crate::domain::LocalIdentity)
/// produces a draft, `EnvelopeSignerPort` turns it into a signed `Envelope`,
/// and no secret byte ever crosses back.
///
/// The bytes to sign come from [`Envelope::signable_bytes`] itself, not from a
/// second implementation of that layout: the draft holds a real `Envelope`
/// with a placeholder signature, and the signature field is not covered by the
/// signable-bytes layout. So a draft and the envelope it becomes always have
/// byte-identical signing input, and the normative layout in `shared_types`
/// can never drift from a copy kept here. The placeholder never escapes —
/// [`into_signed`](Self::into_signed) overwrites it, and no accessor returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedEnvelope(Envelope);

impl UnsignedEnvelope {
    /// Stand-in occupying the signature field until a signer replaces it.
    const PLACEHOLDER_SIGNATURE: EnvelopeSignature =
        EnvelopeSignature::new([0u8; EnvelopeSignature::LENGTH]);

    /// Drafts an envelope authored by `author`.
    ///
    /// Crate-private on purpose: every draft must come from the local
    /// `LocalIdentity`, so an `UnsignedEnvelope` in hand is always one this
    /// peer is entitled to sign.
    pub(crate) fn draft(
        author: PeerId,
        version: ProtocolVersion,
        kind: PayloadKind,
        payload: Vec<u8>,
    ) -> Self {
        Self(Envelope {
            version,
            kind,
            author,
            payload,
            signature: Self::PLACEHOLDER_SIGNATURE,
        })
    }

    pub const fn author(&self) -> PeerId {
        self.0.author
    }

    pub const fn version(&self) -> ProtocolVersion {
        self.0.version
    }

    pub const fn kind(&self) -> PayloadKind {
        self.0.kind
    }

    pub fn payload(&self) -> &[u8] {
        &self.0.payload
    }

    /// The exact bytes a signer must sign — identical to the
    /// [`Envelope::signable_bytes`] of the envelope this draft becomes.
    pub fn signable_bytes(&self) -> Vec<u8> {
        self.0.signable_bytes()
    }

    /// Completes the draft with the signature produced over
    /// [`signable_bytes`](Self::signable_bytes).
    pub fn into_signed(self, signature: EnvelopeSignature) -> Envelope {
        Envelope {
            signature,
            ..self.0
        }
    }
}
