use std::fmt;

use crate::domain::DurationMillis;

/// A point in time in milliseconds, from one peer's own clock.
///
/// This is the `messaging` domain's only notion of time (D11, S5), and it is
/// deliberately not `std::time::Instant` or `SystemTime`: neither can be
/// constructed at a chosen value in a test, and both invite code that reads a
/// clock where it stands rather than receiving a reading from its caller. Only
/// `ClockPort` produces one.
///
/// # Two readings that must never be confused
///
/// A message carries the *author's claim* about when it was sent — recorded for
/// display and used for nothing else, because it is another peer's clock,
/// unsynchronised with this one and freely falsifiable. A buffered arrival also
/// carries the **local** instant it reached this peer, and that one is a fact
/// this peer observed. Only the local instant may drive a rule: rule R's
/// gap-tolerance window ages a gap by how long *this peer* has waited, never by
/// what the author claimed, or an author could keep a gap open forever by
/// backdating.
///
/// Ordering within a conversation is decided by
/// [`SequenceNumber`](crate::domain::SequenceNumber) alone (invariant 5, AC8);
/// `Ord` here exists because a *single* peer's own successive readings are
/// comparable, which is what `ClockPort`'s monotonicity contract is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Millis(u64);

impl Millis {
    /// The origin of the timeline.
    pub const ZERO: Self = Self(0);

    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    /// How long ago `earlier` was, measured from this instant.
    ///
    /// Saturates at zero: a reading that precedes `earlier` would mean the
    /// clock ran backwards, which this context treats as "no age yet" rather
    /// than wrapping into an enormous span that would abandon every open gap at
    /// once.
    pub const fn saturating_elapsed_since(self, earlier: Self) -> DurationMillis {
        DurationMillis::from_millis(self.0.saturating_sub(earlier.0))
    }
}

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms", self.0)
    }
}
