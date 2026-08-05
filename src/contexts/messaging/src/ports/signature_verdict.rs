/// The outcome of checking an envelope's signature against its author.
///
/// A bad signature is an *expected* answer, not an error: hostile and corrupted
/// input is normal on an open network, so
/// [`EnvelopeVerifierPort`](crate::ports::EnvelopeVerifierPort) returns this
/// verdict rather than failing. Callers count `Invalid` for local diagnostics
/// (AC6) while treating a genuine inability to check as something else entirely
/// — see [`EnvelopeVerifierError`](crate::ports::EnvelopeVerifierError).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureVerdict {
    /// The signature verifies against the envelope's author, which is what
    /// makes that peer the message's author (invariant 4).
    Valid,
    /// The signature does not verify; the content must never reach a read
    /// model (invariant 10).
    Invalid,
}

impl SignatureVerdict {
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}
