use std::fmt;

use sha2::{Digest, Sha256};

use crate::PeerId;

/// A human-comparable digest of a [`PeerId`] for out-of-band verification
/// (canvas §2.1): two users read their fingerprints to each other over a
/// separate channel to upgrade a peer from `Unverified` to `Verified`.
///
/// The digest is the SHA-256 of the peer's 32 Ed25519 public-key bytes.
/// Equality and hashing use the full 32-byte digest.
///
/// # Rendering (stable format — pinned by test)
///
/// `Display` renders the **first 16 bytes** (128 bits) of the digest as
/// 8 space-separated groups of 4 lowercase hex characters, e.g.:
///
/// ```text
/// 21fe 31df a154 a261 626b f854 046f d227
/// ```
///
/// 128 bits keeps the string short enough to read aloud while an attacker
/// would need a second preimage of the displayed prefix (~2¹²⁸ work) to
/// impersonate a fingerprint someone has already noted down. This format is
/// a compatibility contract: changing it invalidates every fingerprint users
/// have already compared, so it must never change silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; Self::LENGTH]);

impl Fingerprint {
    /// Byte length of the full digest.
    pub const LENGTH: usize = 32;

    /// Number of digest bytes shown by `Display`.
    const DISPLAY_BYTES: usize = 16;

    /// Computes the fingerprint of a peer's public key.
    pub fn of(peer: &PeerId) -> Self {
        let digest = Sha256::digest(peer.as_bytes());
        Self(digest.into())
    }

    /// The full 32-byte SHA-256 digest.
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, pair) in self.0[..Self::DISPLAY_BYTES].chunks(2).enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{:02x}{:02x}", pair[0], pair[1])?;
        }
        Ok(())
    }
}
