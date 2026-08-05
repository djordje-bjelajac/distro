use std::fmt;

/// The text a message carries: UTF-8, non-empty, and bounded (canvas §2.3).
///
/// Trimming happens before measuring, so leading and trailing whitespace never
/// costs a sender their message, and a body that is *only* whitespace is empty
/// rather than a message of blanks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageBody(String);

impl MessageBody {
    /// A message must say something.
    pub const MIN_BYTES: usize = 1;

    /// 16 KiB, in bytes — safeguard S6's body cap, stated here in the domain
    /// so it holds for locally composed messages too, not only for what
    /// arrives from the wire.
    ///
    /// The figure is a hostile-input bound rather than a product decision: a
    /// symmetric open network has no gatekeeper who could add one later, and
    /// combined with the per-author out-of-order buffer cap it puts a hard
    /// ceiling on the memory one peer can make another hold.
    pub const MAX_BYTES: usize = 16 * 1024;

    /// Trims `text` and accepts it if what remains fits the bounds.
    pub fn new(text: &str) -> Result<Self, MessageBodyError> {
        let trimmed = text.trim();

        if trimmed.len() < Self::MIN_BYTES {
            return Err(MessageBodyError::Empty);
        }
        if trimmed.len() > Self::MAX_BYTES {
            return Err(MessageBodyError::TooLong {
                bytes: trimmed.len(),
            });
        }

        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Size in **bytes**, which is what the cap counts: a body of multi-byte
    /// scalars holds fewer characters than an ASCII body of the same size.
    pub fn len_bytes(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for MessageBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Typed rejection of a [`MessageBody`] construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageBodyError {
    /// Nothing remained once the text was trimmed.
    Empty,
    /// The trimmed text exceeds [`MessageBody::MAX_BYTES`].
    TooLong {
        /// The trimmed size that was offered, for the diagnostic.
        bytes: usize,
    },
}

impl fmt::Display for MessageBodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a message body is empty once trimmed"),
            Self::TooLong { bytes } => write!(
                f,
                "a message body of {bytes} bytes exceeds the {}-byte limit",
                MessageBody::MAX_BYTES
            ),
        }
    }
}

impl std::error::Error for MessageBodyError {}
