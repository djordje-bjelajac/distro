use std::fmt::{self, Write as _};

use shared_types::{Fingerprint, PeerId};

/// A human-readable label a peer chooses for itself (canvas §2.1).
///
/// Invariant: 1–64 Unicode scalar values after trimming, with no control
/// character anywhere in the trimmed text.
///
/// The limit is counted in **Unicode scalar values**, not bytes and not
/// grapheme clusters: bytes would penalise non-Latin scripts, and grapheme
/// segmentation is unbounded per grapheme (a single emoji ZWJ sequence can
/// carry arbitrarily many scalar values), which would leave the storage and
/// wire cost of a name unbounded. Scalar values are the one measure that is
/// both script-neutral and bounded.
///
/// Canvas invariant 8: a `DisplayName` never participates in identity,
/// equality of peers, addressing, or lookup — it is decoration on top of a
/// [`PeerId`], and two peers may freely choose the same one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

impl DisplayName {
    /// Fewest Unicode scalar values a display name may have after trimming.
    pub const MIN_SCALAR_VALUES: usize = 1;

    /// Most Unicode scalar values a display name may have after trimming.
    pub const MAX_SCALAR_VALUES: usize = 64;

    /// Prefix of the name derived by [`derived_from`](Self::derived_from).
    const DERIVED_PREFIX: &'static str = "peer-";

    /// Fingerprint bytes rendered into a derived name.
    const DERIVED_DIGEST_BYTES: usize = 4;

    /// Validates `raw` into a display name.
    ///
    /// Leading and trailing whitespace is trimmed first, so a name pasted
    /// with padding is accepted and stored in its trimmed form; the length
    /// limit is then measured on the trimmed text. Control characters are
    /// rejected wherever they appear in the trimmed text — they would let a
    /// name overwrite terminal output or forge UI structure. Trimming before
    /// that check is deliberate: a trailing newline is padding, not an
    /// attempt to smuggle a control character.
    pub fn new(raw: &str) -> Result<Self, DisplayNameError> {
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(DisplayNameError::ContainsControlCharacter);
        }

        let scalar_values = trimmed.chars().count();
        if scalar_values > Self::MAX_SCALAR_VALUES {
            return Err(DisplayNameError::TooLong {
                scalar_values,
                limit: Self::MAX_SCALAR_VALUES,
            });
        }

        Ok(Self(trimmed.to_owned()))
    }

    /// The zero-interaction default for a fresh install (AC1): a name derived
    /// deterministically from the peer's own fingerprint, e.g. `peer-d75a9801`.
    ///
    /// A `LocalIdentity` always carries a valid display name, and first launch
    /// asks the user nothing — so *something* must fill the field. Deriving it
    /// from the fingerprint keeps the default recognisable, stable across
    /// restarts, and free of any claim about who the peer is. Construction
    /// cannot fail: the result is 13 scalar values of prefix and hex.
    pub fn derived_from(peer: &PeerId) -> Self {
        let digest = Fingerprint::of(peer);
        let mut name = String::from(Self::DERIVED_PREFIX);
        for byte in &digest.as_bytes()[..Self::DERIVED_DIGEST_BYTES] {
            let _ = write!(name, "{byte:02x}");
        }
        Self(name)
    }

    /// The trimmed, validated text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Typed construction error for [`DisplayName`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayNameError {
    /// The name is empty once surrounding whitespace is removed.
    Empty,
    /// The trimmed name contains a control character.
    ContainsControlCharacter,
    /// The trimmed name exceeds [`DisplayName::MAX_SCALAR_VALUES`].
    TooLong {
        /// Unicode scalar values counted in the trimmed name.
        scalar_values: usize,
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for DisplayNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("display name is empty after trimming"),
            Self::ContainsControlCharacter => {
                f.write_str("display name contains a control character")
            }
            Self::TooLong {
                scalar_values,
                limit,
            } => write!(
                f,
                "display name is {scalar_values} Unicode scalar values, limit is {limit}"
            ),
        }
    }
}

impl std::error::Error for DisplayNameError {}
