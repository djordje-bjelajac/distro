use shared_types::PeerId;

use crate::domain::events::PeerPresenceExpired;
use crate::domain::{Endpoint, SessionOutcome};
use crate::ports::{DiscoveredPeer, DiscoveryOutcome, EventPublisherError, MembershipCommandError};

/// The **inbound** (driving) contract the network runtime calls (canvas §4,
/// inbound column; S3).
///
/// This is the boundary S3 names: everything the outside world reports about
/// peers enters `membership` here, and an adapter never reaches past it into
/// the roster. What arrives is always a *report* — a peer announced itself, a
/// remote dialled us, a handshake completed, a link died — never a decision.
/// The decisions are [`JoinNetworkPort`](crate::ports::JoinNetworkPort)'s.
///
/// # Why the presence sweep lives here too
///
/// [`expire_presence`](Self::expire_presence) is the "and nothing arrived"
/// half of the same story. Presence is derived from the evidence that reaches
/// this port (invariant 7), so the absence of evidence is evaluated at the
/// same boundary, driven by the same runtime tick. Putting it on the query
/// side instead would make a redraw mutate state; putting it on the decision
/// side would suggest a user asked for it.
///
/// # Direction is implicit
///
/// Sessions reported here are always **inbound**: a remote dialled us. An
/// outbound dial is a decision, and it is made through `JoinNetworkPort`. That
/// is why no method takes a `SessionDirection` — a transport reporting the
/// wrong direction is a class of bug this signature deletes.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn InboundSessionPort + Send + Sync>`.
pub trait InboundSessionPort {
    /// A discovery mechanism saw `peer` at the addresses it claims.
    ///
    /// Claims, not facts: an announcement is asserted by whoever made it, and
    /// nothing is proven until the session handshake. The local peer's own
    /// announcement coming back is reported as
    /// [`DiscoveryOutcome::OwnAnnouncement`], not as an error.
    fn peer_observed(
        &self,
        discovered: DiscoveredPeer,
    ) -> Result<DiscoveryOutcome, MembershipCommandError>;

    /// A remote peer dialled this one; the link is up but not yet
    /// authenticated.
    ///
    /// `endpoints` is where the link came from, which is how a peer that
    /// redeemed *our* join ticket becomes known to us at all — it dialled
    /// before we had ever discovered it, so this call has to be able to enter
    /// it in the roster.
    ///
    /// The returned [`SessionOutcome`] may name a session the collapse rule
    /// discarded (invariant 3). Closing that link is the caller's job: the
    /// transport port closes *by peer*, and only the adapter that accepted the
    /// two links can tell them apart.
    fn session_opened(
        &self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
    ) -> Result<SessionOutcome, MembershipCommandError>;

    /// The authenticated handshake with `peer` completed.
    ///
    /// The only moment `PeerConnected` is published — the moment the peer
    /// actually becomes reachable, which is what other contexts act on.
    fn session_established(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError>;

    /// The link to `peer` ended, for any reason the transport observed.
    ///
    /// Publishes `PeerDisconnected` only if the session had established: a
    /// session that died while connecting was never announced, and an
    /// unmatched disconnect would make `messaging` fail directs for a peer it
    /// never considered reachable (D10).
    ///
    /// The transport is not asked to close anything — it is the one reporting.
    fn session_closed(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError>;

    /// `peer` produced evidence of life: a keep-alive, or any traffic at all.
    ///
    /// The peer must already be known; evidence with no address to dial is not
    /// something this context can act on.
    fn peer_heartbeat(&self, peer: PeerId) -> Result<(), MembershipCommandError>;

    /// Re-derives every peer's presence against the clock and announces those
    /// that have newly fallen silent (AC5).
    ///
    /// Idempotent within one silence: a peer is reported once per stretch of
    /// quiet, and fresh evidence re-arms it. Sessions are untouched — silence
    /// is not a closed link, and whether an expiry should provoke one is a
    /// decision, not an observation.
    fn expire_presence(&self) -> Result<Vec<PeerPresenceExpired>, EventPublisherError>;
}
