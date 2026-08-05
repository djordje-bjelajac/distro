use std::fmt;

/// An author's position in one conversation: strictly monotonic per
/// `(author, conversation)` (invariant 5).
///
/// This — never a timestamp — is what orders a conversation. Each author counts
/// their own messages in each conversation independently, so AC8 ("messages
/// from one author display in that author's send order regardless of arrival
/// order") is decidable from the message alone, with no clock, no consensus,
/// and no global counter.
///
/// # Why counting starts at 1
///
/// 0 is reserved for "no message yet". A high-water mark is an `Option` in
/// this crate, but the number also crosses the wire and reaches diagnostics
/// and codecs that have no option type; keeping 0 out of the value space means
/// an absent mark can never be mistaken for a real first message. A received 0
/// is therefore a typed rejection, not a silently accepted message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceNumber(u64);

impl SequenceNumber {
    /// The number the first message of an author in a conversation carries.
    pub const FIRST: Self = Self(1);

    /// The last representable number; it has no successor.
    pub const MAX: Self = Self(u64::MAX);

    /// Builds a sequence number from a raw value, rejecting the reserved 0.
    pub const fn new(value: u64) -> Result<Self, SequenceNumberError> {
        if value == 0 {
            return Err(SequenceNumberError::Zero);
        }

        Ok(Self(value))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// The next number after this one.
    ///
    /// Fails rather than wrapping: a wrapped counter would silently re-issue
    /// numbers an author already used, and every dedup and ordering rule in
    /// this context assumes that never happens.
    pub const fn successor(self) -> Result<Self, SequenceNumberError> {
        match self.0.checked_add(1) {
            Some(next) => Ok(Self(next)),
            None => Err(SequenceNumberError::Exhausted),
        }
    }

    /// The number before this one; `None` for [`FIRST`](Self::FIRST), which has
    /// none.
    ///
    /// Abandoning a gap needs it: the log's mark moves to the number *below*
    /// the lowest message it is holding, which is the last sequence it is
    /// giving up on (rule R).
    pub const fn predecessor(self) -> Option<Self> {
        match self.0 {
            0 | 1 => None,
            value => Some(Self(value - 1)),
        }
    }

    /// The number that must come after `previous` — [`FIRST`](Self::FIRST) when
    /// there is no previous message.
    ///
    /// This is the single definition of "contiguous", shared by the outbound
    /// path (which assigns it) and the inbound path (which compares against
    /// it), so the two can never drift apart.
    pub const fn following(previous: Option<Self>) -> Result<Self, SequenceNumberError> {
        match previous {
            None => Ok(Self::FIRST),
            Some(previous) => previous.successor(),
        }
    }
}

impl fmt::Display for SequenceNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Typed rejection of a [`SequenceNumber`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceNumberError {
    /// 0 is reserved for "no message yet" and is not a sequence number.
    Zero,
    /// The author has used every representable number in this conversation.
    Exhausted,
}

impl fmt::Display for SequenceNumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => f.write_str("0 is not a sequence number; the first message carries 1"),
            Self::Exhausted => f.write_str("no sequence number remains after the last one"),
        }
    }
}

impl std::error::Error for SequenceNumberError {}
