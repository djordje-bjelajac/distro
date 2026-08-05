use std::fmt;

use ed25519_dalek::VerifyingKey;

/// A peer's network-wide identity: its Ed25519 public key.
///
/// Invariant (canvas §2.5/1): a `PeerId` can only be constructed from bytes
/// that decode to a valid Ed25519 public key, so an identity–key mismatch is
/// unrepresentable. Equality, ordering, and hashing are all defined by the
/// key bytes; the lexicographic `Ord` is what the session-collapse rule
/// (canvas §2.5/3) relies on, so it must never change.
///
/// No `ed25519-dalek` type appears in this API; validation is an internal
/// construction detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId([u8; Self::LENGTH]);

impl PeerId {
    /// Byte length of an Ed25519 public key.
    pub const LENGTH: usize = 32;

    /// Constructs a `PeerId` from raw Ed25519 public-key bytes, rejecting any
    /// byte string that is not a valid public-key encoding.
    pub fn from_public_key_bytes(bytes: [u8; Self::LENGTH]) -> Result<Self, PeerIdError> {
        VerifyingKey::from_bytes(&bytes)
            .map(|_| Self(bytes))
            .map_err(|_| PeerIdError::InvalidPublicKey)
    }

    /// The raw Ed25519 public-key bytes this identity wraps.
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

/// Typed construction error for [`PeerId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerIdError {
    /// The provided bytes are not a valid Ed25519 public-key encoding.
    InvalidPublicKey,
}

impl fmt::Display for PeerIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPublicKey => f.write_str("bytes are not a valid Ed25519 public key"),
        }
    }
}

impl std::error::Error for PeerIdError {}
