/// Where a remote peer stands in the trust-on-first-use ladder (canvas §2.1,
/// D5).
///
/// The ladder is monotonic: `Unverified → Verified` is the only transition,
/// and there is no way back. Un-verifying would mean asserting that a key
/// comparison a user performed out-of-band never happened; if a peer's key
/// changes, that is a *different* `PeerId` and therefore a different
/// [`TrustRecord`](crate::domain::TrustRecord), not a downgrade of this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerificationState {
    /// Seen, but its key has never been confirmed out-of-band.
    #[default]
    Unverified,
    /// Its fingerprint was compared out-of-band and matched.
    Verified,
}

impl VerificationState {
    pub const fn is_verified(&self) -> bool {
        matches!(self, Self::Verified)
    }
}
