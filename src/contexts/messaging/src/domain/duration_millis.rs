use std::fmt;

/// A span of time in whole milliseconds: the distance between two
/// [`Millis`](crate::domain::Millis) readings.
///
/// Deliberately a separate type from the instant it measures. This context has
/// exactly one time-dependent rule — how long a gap may stay open before it is
/// abandoned (invariant 5, rule R) — and a bare `u64` would let a caller
/// compare that window against a wall-clock reading, or subtract one from the
/// other, without the compiler noticing.
///
/// # A duplicate by design
///
/// `membership` declares its own `DurationMillis`. `shared_types` is a
/// data-contract crate (canvas §2.4) and contexts never import each other
/// (canvas §4), so each states the time it needs in its own terms.
///
/// Milliseconds are the unit throughout: coarse enough that no value is ever
/// fractional, fine enough for the gap-tolerance window, and `u64`
/// milliseconds cover ~584 million years, so the saturating arithmetic below is
/// a formality rather than a live concern.
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
