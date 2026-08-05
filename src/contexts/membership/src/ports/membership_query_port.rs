use shared_types::PeerId;

use crate::domain::NetworkStatus;
use crate::ports::KnownPeerView;

/// The **inbound** (driving) read contract of `membership` (canvas §4, inbound
/// column).
///
/// Every method reads and returns; none writes. That is the half of the CQRS
/// split `AGENTS.md` requires be kept separate end to end, and here it carries
/// a second weight: [`Presence`](crate::domain::Presence) is *derived* from
/// evidence age at read time (invariant 7), so a query that wrote would be
/// promoting a derivation to a stored fact. This crate's query tests assert
/// the roster is unchanged after any number of reads rather than trusting the
/// convention.
///
/// # No `Result`
///
/// Nothing here can fail. The roster lives in memory, and reading it needs no
/// store, no socket, and no fallible parse — only the clock, which cannot
/// fail either (see `ClockPort`). Giving these methods a `Result` would push a
/// meaningless error branch into every redraw.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn MembershipQueryPort + Send + Sync>`.
pub trait MembershipQueryPort {
    /// Every peer this instance knows about, in `PeerId` order, each with the
    /// presence derived at the moment of the call.
    fn known_peers(&self) -> Vec<KnownPeerView>;

    /// The peers whose evidence of life is fresh enough to be `Online`, in
    /// `PeerId` order.
    ///
    /// Online is **not** connected: a peer seen announcing itself a second ago
    /// is online with no session at all, and a peer holding an established
    /// session goes stale and then offline if it stops speaking. Callers that
    /// mean "can I reach it right now" want the `is_connected` flag on
    /// [`known_peers`](Self::known_peers).
    fn online_peers(&self) -> Vec<PeerId>;

    /// How connected this instance currently is —
    /// `Isolated`, `Joining`, or `Connected(n)`.
    ///
    /// `Joining` is reported for exactly as long as a bootstrap ladder is in
    /// flight, which no count of sessions could ever tell the caller.
    fn network_status(&self) -> NetworkStatus;
}
