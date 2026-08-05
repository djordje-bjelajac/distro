use std::fmt;

use crate::domain::DurationMillis;

/// A point on a monotonic timeline, in milliseconds since an arbitrary origin.
///
/// This is the `membership` domain's only notion of time (D11, S5). It is
/// deliberately *not* `std::time::Instant` or `SystemTime`: neither can be
/// constructed at a chosen value in a test, both invite code that reads the
/// clock where it stands rather than receiving a reading from the caller, and
/// `SystemTime` can jump backwards. Every time-dependent rule in this context
/// therefore takes a `Millis` argument, and only `ClockPort` produces one.
///
/// The origin is unspecified on purpose: differences between two readings are
/// meaningful, an absolute value is not. Comparing readings from two different
/// peers is meaningless and no rule in this context does it — presence,
/// expiry, and ticket validity are all evaluated against the *local* clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Millis(u64);

impl Millis {
    /// The origin of the timeline.
    pub const ZERO: Self = Self(0);

    /// The far end of the timeline; a ticket expiring here effectively never
    /// expires.
    pub const MAX: Self = Self(u64::MAX);

    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// How long ago `earlier` was, measured from this instant.
    ///
    /// Saturates at zero: a reading that precedes `earlier` would mean the
    /// monotonic clock ran backwards, which this context treats as "no age
    /// yet" rather than wrapping into an enormous span that would flip every
    /// peer to `Offline` at once.
    pub const fn saturating_elapsed_since(self, earlier: Self) -> DurationMillis {
        DurationMillis::from_millis(self.0.saturating_sub(earlier.0))
    }

    /// The instant `span` after this one, clamped at [`Millis::MAX`].
    pub const fn saturating_add(self, span: DurationMillis) -> Self {
        Self(self.0.saturating_add(span.as_millis()))
    }
}

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms", self.0)
    }
}
