use std::fmt;

/// Lifecycle position of a [`Session`](crate::domain::Session) (canvas §2.2).
///
/// `Closed` is terminal. A dropped link is never revived in place — the
/// application opens a fresh session — because reusing a closed session would
/// make "is this the same link the remote thinks it has?" unanswerable, which
/// is exactly the question the collapse rule (invariant 3) depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Dialling or being dialled; not yet authenticated.
    Connecting,
    /// Authenticated and usable.
    Established,
    /// Ended, for any reason. Terminal.
    Closed,
}

impl SessionState {
    /// Whether the session still occupies the slot for its peer.
    ///
    /// Both pre-terminal states count: a `Connecting` session is already a
    /// commitment to one link, which is why a second dial in the same
    /// direction is a conflict rather than a retry.
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Connecting | Self::Established)
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting => f.write_str("connecting"),
            Self::Established => f.write_str("established"),
            Self::Closed => f.write_str("closed"),
        }
    }
}
