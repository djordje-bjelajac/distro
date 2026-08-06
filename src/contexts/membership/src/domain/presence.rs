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
/// # `Unknown` is not a rung on the ladder
///
/// `Online → Stale → Offline` is an ageing ladder over **one measurement**: the
/// instant of the last evidence. `Stale` is the interval in which the local
/// view honestly does not know yet, which is what keeps a UI from claiming a
/// departure it cannot justify.
///
/// `Unknown` is not a fourth rung on that ladder — it is the absence of the
/// measurement the ladder ages. A peer learned from the cache, from an mDNS
/// sighting, or from a DHT record has produced nothing; there is no age to
/// derive from, and inventing one (`Millis::ZERO`, or the instant we *heard
/// about* it) fabricates the input rather than reporting it. Its only exit is
/// evidence, and it is never on the path to `Offline`: `Offline` is the
/// negative claim "we were talking and they went away", which cannot be true of
/// a peer that never spoke (canvas D1, invariant 4).
///
/// # No `Ord`
///
/// The absence of `PartialOrd`/`Ord` is deliberate and load-bearing. Nothing in
/// the workspace orders presences, and the ladder's apparent order is exactly
/// the trap: a later `>= Presence::Stale` or a `max()` over a roster would fold
/// `Unknown` back into a verdict about liveness, which is the defect this type
/// exists to prevent. Callers that want a classification ask for one
/// ([`is_online`](Self::is_online), [`is_offline`](Self::is_offline),
/// [`is_unknown`](Self::is_unknown)) or match exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Presence {
    /// No evidence of life has ever arrived for this peer: it is known only
    /// because something *else* named it. Distinct from `Offline`, and
    /// differently actionable — an address we hold and have never reached is
    /// worth dialling, a peer that went quiet is not news.
    Unknown,
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
    /// Classifies a peer from the age of its last evidence of life, or reports
    /// [`Unknown`](Self::Unknown) when there has never been any.
    ///
    /// `None` is not a degenerate case to be smoothed over: it is the state
    /// every peer starts in, and it is the answer this derivation must give
    /// before any input exists. Taking `Option<Millis>` rather than a `Millis`
    /// some constructor had to invent is what makes the fabricated instant
    /// unrepresentable (canvas D1).
    ///
    /// Both windows are half-open: age `< online` is `Online`, age `< offline`
    /// is `Stale`, anything else is `Offline`. Half-open intervals give every
    /// age exactly one classification with no boundary shared between two
    /// verdicts, so the truth table has no ambiguous row.
    ///
    /// A `now` that precedes the evidence — only possible if a clock ran
    /// backwards — yields an age of zero, i.e. `Online`, rather than an
    /// enormous wrapped span that would silently mark the whole roster offline.
    pub const fn derive(
        last_evidence_at: Option<Millis>,
        now: Millis,
        windows: LivenessWindows,
    ) -> Self {
        let Some(last_evidence_at) = last_evidence_at else {
            return Self::Unknown;
        };

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

    /// Whether this is the negative verdict.
    ///
    /// False for [`Unknown`](Self::Unknown): never having heard from a peer is
    /// not a claim that it is gone, and the two must not collapse into one
    /// answer for any caller — expiry least of all (invariant 5).
    pub const fn is_offline(self) -> bool {
        matches!(self, Self::Offline)
    }

    /// Whether no evidence has ever arrived for this peer.
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

impl fmt::Display for Presence {
    /// The diagnostic label. The roster pane renders `Unknown` as a blank cell
    /// rather than this word (canvas §3, OP-7) — a column of "unknown" reads as
    /// a fault, when the honest statement is that there is nothing to report.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown"),
            Self::Online => f.write_str("online"),
            Self::Stale => f.write_str("stale"),
            Self::Offline => f.write_str("offline"),
        }
    }
}
