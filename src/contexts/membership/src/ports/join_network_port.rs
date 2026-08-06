use shared_types::PeerId;

use crate::domain::{JoinTicket, SessionOutcome};
use crate::ports::{
    EventPublisherError, ForgetPeersError, ForgetPeersOutcome, JoinOutcome, LeaveOutcome,
    MembershipCommandError,
};

/// The **inbound** (driving) contract for the deliberate decisions this peer
/// makes about its own membership (canvas §4, inbound column).
///
/// Everything here is something a *person* or a startup step decided: join,
/// leave, connect to that peer. What the network decides — a remote dialled us,
/// a link dropped, a peer went quiet — arrives on
/// [`InboundSessionPort`](crate::ports::InboundSessionPort) instead. Keeping
/// the two apart is what stops a UI action and a wire event from sharing a code
/// path in which neither's failure mode makes sense.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn JoinNetworkPort + Send + Sync>`.
pub trait JoinNetworkPort {
    /// Walks the D1 bootstrap ladder — cached peers, then the local network,
    /// then the supplied ticket — and stops at the first peer that answers.
    ///
    /// # This never hangs and never fails silently (AC3)
    ///
    /// Every rung is bounded: it either connects, or reports why not into the
    /// returned [`JoinOutcome::diagnostic`]. A walk in which no rung connects
    /// ends at `Isolated`, which is a normal state and therefore `Ok` — the
    /// caller distinguishes the two by [`JoinOutcome::succeeded`], and shows
    /// the diagnostic either way.
    ///
    /// `ticket` is validated at its own rung rather than up front, on purpose:
    /// a stale ticket in a config file must not stop a machine with a warm
    /// cache from joining, and an expired one is only worth reporting if it
    /// was actually needed.
    ///
    /// The one `Err` is the event publisher: the peer may have connected while
    /// no consumer was told, which no diagnostic can paper over.
    fn join_network(&self, ticket: Option<JoinTicket>) -> Result<JoinOutcome, EventPublisherError>;

    /// Closes every session, saves the peer cache for the next launch's first
    /// bootstrap rung, and announces the departure.
    ///
    /// In that order: a consumer must never see the network left while it
    /// still believes a session is live.
    fn leave_network(&self) -> Result<LeaveOutcome, EventPublisherError>;

    /// Dials a peer this instance already knows, on purpose — the UI's
    /// "connect" action, and the same step the bootstrap ladder takes for each
    /// candidate.
    ///
    /// The peer must already be in the roster: a dial needs an address, and an
    /// address is what discovery is for. Unknown peers are rejected with
    /// [`PeerRosterError::UnknownPeer`](crate::domain::PeerRosterError::UnknownPeer)
    /// rather than silently ignored.
    fn connect_to_peer(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError>;

    /// Leaves the network and then forgets every peer, so the next launch is a
    /// genuine cold start.
    ///
    /// # It leaves first, and the order is the whole operation
    ///
    /// Sessions live *inside* roster entries. Emptying the roster first would
    /// leave the transport holding every link with nothing left to close them
    /// by, and the next inbound frame would recreate the entry through
    /// discovery — the peer the user asked to forget, back within seconds.
    /// So this closes every session and announces every departure exactly as
    /// [`leave_network`](Self::leave_network) does, *then* empties the roster,
    /// *then* writes an empty cache.
    ///
    /// The intermediate save that a leave performs is deliberately left in
    /// place: it buys one code path for "close everything" rather than a
    /// second, nearly identical one, and the empty write that follows is the
    /// one that lasts.
    ///
    /// # What it does not touch
    ///
    /// Trust records, the identity keypair, and the outbound sequence counter
    /// are all somebody else's, and forgetting a peer is not a reason to
    /// unblock it, change identity, or go mute. Nothing in this call reaches
    /// them.
    fn forget_known_peers(&self) -> Result<ForgetPeersOutcome, ForgetPeersError>;
}
