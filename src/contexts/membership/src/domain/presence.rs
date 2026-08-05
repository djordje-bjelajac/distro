use std::fmt;

use crate::domain::{LivenessWindows, Millis};

/// What the local peer currently believes about a remote peer's reachability
/// (canvas §2.2).
///
/// **Derived, never asserted** (invariant 7). `Presence` is not state the
/// roster sets; it is a pure function of how old the last evidence of life is
/// — [`derive`](Self::derive) is total, deterministic, and reads no clock. The
/// roster stores the evidence instant; the presence follows from it and from
/// whatever `now` the caller obtained from `ClockPort` (D11, S5).
///
/// Consequently no peer ever tells another peer who is online: each view is
/// authoritative only for itself (invariant 9), and a peer that looks `Offline`
/// here may be perfectly healthy behind a broken path.
///
/// The three values are a deliberate ladder rather than a boolean: `Stale` is
/// the interval in which the local view honestly does not know, which is what
/// keeps a UI from claiming a departure it cannot yet justify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Presence {
    /// Evidence is younger than the online window.
    Online,
    /// Evidence has aged past the online window but not past the offline one:
    /// the peer may be gone, may merely be quiet.
    Stale,
    /// Evidence is at least as old as the offline window: treat the peer as
    /// gone until it proves otherwise.
    Offline,
}

impl Presence {
    /// Classifies a peer from the age of its last evidence of life.
    ///
    /// Both windows are half-open: age `< online` is `Online`, age `< offline`
    /// is `Stale`, anything else is `Offline`. Half-open intervals give every
    /// age exactly one classification with no boundary shared between two
    /// verdicts, so the truth table has no ambiguous row.
    ///
    /// A `now` that precedes the evidence — only possible if a clock ran
    /// backwards — yields an age of zero, i.e. `Online`, rather than an
    /// enormous wrapped span that would silently mark the whole roster offline.
    pub const fn derive(last_evidence_at: Millis, now: Millis, windows: LivenessWindows) -> Self {
        let age = now.saturating_elapsed_since(last_evidence_at).as_millis();

        if age < windows.online().as_millis() {
            Self::Online
        } else if age < windows.offline().as_millis() {
            Self::Stale
        } else {
            Self::Offline
        }
    }

    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online)
    }

    pub const fn is_offline(self) -> bool {
        matches!(self, Self::Offline)
    }
}

impl fmt::Display for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Online => f.write_str("online"),
            Self::Stale => f.write_str("stale"),
            Self::Offline => f.write_str("offline"),
        }
    }
}
