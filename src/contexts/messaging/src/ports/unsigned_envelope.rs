use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

/// An [`Envelope`] this peer has drafted but not yet signed.
///
/// It exists so signing can be a port operation without any key material
/// entering this crate: the application drafts, [`EnvelopeSignerPort`](crate::ports::EnvelopeSignerPort)
/// turns the draft into a signed `Envelope`, and no secret byte moves in either
/// direction.
///
/// # The signing input cannot drift
///
/// The draft holds a real `Envelope` with a placeholder signature, and the
/// signature field is not covered by
/// [`Envelope::signable_bytes`](shared_types::Envelope::signable_bytes). So a
/// draft and the envelope it becomes have byte-identical signing input, and the
/// normative layout in `shared_types` can never diverge from a second copy kept
/// here — because there is no second copy. The placeholder never escapes:
/// [`into_signed`](Self::into_signed) overwrites it and no accessor returns it.
///
/// # Why this lives in `ports` and not `domain`
///
/// An envelope is a wire contract, not a conversation concept. The domain
/// models messages and ordering and has no idea that anything is ever
/// encoded — only the seam where a message leaves this peer needs an envelope,
/// and that seam is this port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsignedEnvelope(Envelope);

impl UnsignedEnvelope {
    /// Stand-in occupying the signature field until a signer replaces it.
    const PLACEHOLDER_SIGNATURE: EnvelopeSignature =
        EnvelopeSignature::new([0u8; EnvelopeSignature::LENGTH]);

    /// Drafts an envelope to be authored by `author`.
    ///
    /// `author` must be the local peer. Nothing here can check that — this
    /// crate holds no identity — so the signer enforces it instead and returns
    /// [`EnvelopeSignerError::AuthorMismatch`](crate::ports::EnvelopeSignerError::AuthorMismatch)
    /// for a draft naming anyone else. That check is what keeps a drafting
    /// mistake from turning into an envelope that claims a peer's identity.
    pub const fn draft(
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
    /// [`Envelope::signable_bytes`](shared_types::Envelope::signable_bytes) of
    /// the envelope this draft becomes.
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
