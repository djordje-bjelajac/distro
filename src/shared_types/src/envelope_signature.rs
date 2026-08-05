/// The Ed25519 signature carried by an [`Envelope`](crate::Envelope), as a
/// plain 64-byte value.
///
/// This type holds no cryptographic logic: producing a signature over
/// [`Envelope::signable_bytes`](crate::Envelope::signable_bytes) and
/// verifying one are identity-context port contracts (`EnvelopeSignerPort`,
/// `EnvelopeVerifierPort`), never a `shared_types` concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvelopeSignature([u8; Self::LENGTH]);

impl EnvelopeSignature {
    /// Byte length of an Ed25519 signature.
    pub const LENGTH: usize = 64;

    pub const fn new(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// The raw signature bytes.
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}
