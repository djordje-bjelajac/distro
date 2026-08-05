use crate::domain::NetworkStatus;
use crate::domain::events::NetworkJoined;
use crate::ports::JoinDiagnostic;

/// What a walk of the D1 bootstrap ladder produced.
///
/// # There is no failure case
///
/// `join_network` returns this on every path that is not an infrastructure
/// fault, because "nobody answered" is not an error: `Isolated` is a normal
/// state (canvas §2.2) and a fresh install on a quiet network with no ticket
/// is *supposed* to reach it. Modelling it as `Err` would make the ordinary
/// first launch look broken and would tempt a caller into retry logic where
/// none belongs.
///
/// What the caller does get, unconditionally, is a
/// [`diagnostic`](Self::diagnostic) naming every rung tried and why it
/// produced nothing (AC3). Success and failure are told apart by
/// [`joined`](Self::joined) — present exactly when this walk connected a peer
/// that was not already connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinOutcome {
    /// Where the peer stands now the walk is over: `Connected(n)` or
    /// `Isolated`. Never `Joining` — the phase ends with the walk.
    pub status: NetworkStatus,
    /// The event published when this walk reached the network, or `None` when
    /// it connected nobody.
    pub joined: Option<NetworkJoined>,
    /// What was tried, in order, and what came of each attempt.
    pub diagnostic: JoinDiagnostic,
}

impl JoinOutcome {
    /// Whether this walk reached the network.
    pub const fn succeeded(&self) -> bool {
        self.joined.is_some()
    }
}
