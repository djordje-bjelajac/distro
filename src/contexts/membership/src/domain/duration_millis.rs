use std::fmt;

/// A span of time in whole milliseconds: the distance between two
/// [`Millis`](crate::domain::Millis) readings.
///
/// Deliberately a separate type from the instant it measures. Presence windows,
/// ticket lifetimes, and evidence ages are all durations, and a bare `u64`
/// would let a caller subtract a window from an instant, or compare an age
/// against a wall-clock reading, without the compiler noticing.
///
/// Milliseconds are the unit throughout this context: coarse enough that no
/// value is ever fractional, fine enough for every liveness and retry rule the
/// canvas states, and `u64` milliseconds cover ~584 million years, so the
/// saturating arithmetic below is a formality rather than a live concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DurationMillis(u64);

impl DurationMillis {
    /// The empty span.
    pub const ZERO: Self = Self(0);

    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Builds a span from whole seconds, saturating rather than overflowing.
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(1_000))
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DurationMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms", self.0)
    }
}
