use std::fmt;

/// Why a gap was given up on (invariant 5, rule R; AC15).
///
/// A gap means *not yet received*, but only for a bounded interval — after that
/// the log must move on or the author's whole stream stalls behind messages
/// that may never arrive. Two conditions end the wait, and they say very
/// different things about the network:
///
/// - [`ToleranceElapsed`](Self::ToleranceElapsed) is the ordinary one: the
///   missing messages did not turn up in time.
/// - [`BufferFull`](Self::BufferFull) is the resource one: one author filled
///   this peer's per-author buffer while a gap stayed open (S6). It is a
///   symptom of volume — possibly deliberate volume — rather than of latency.
///
/// Both close the *same* gap in the same way. Naming the cause is what lets a
/// diagnostic tell a slow path from a flooding peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GapCloseCause {
    /// The gap-tolerance window elapsed with the gap still open.
    ToleranceElapsed,
    /// The author's out-of-order buffer had no room left, so the oldest gap was
    /// abandoned to make the held messages visible — rather than refusing the
    /// arrival and losing it.
    BufferFull,
}

impl fmt::Display for GapCloseCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ToleranceElapsed => f.write_str(
                "the gap-tolerance window elapsed without the missing messages arriving",
            ),
            Self::BufferFull => f.write_str("the author's out-of-order buffer was full"),
        }
    }
}
