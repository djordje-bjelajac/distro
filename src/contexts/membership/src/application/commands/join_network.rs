use std::num::NonZeroUsize;
use std::sync::Arc;

use shared_types::PeerId;

use crate::application::commands::{
    EstablishSession, EstablishSessionHandler, OpenSession, OpenSessionHandler,
};
use crate::application::{MembershipSettings, MembershipState};
use crate::domain::events::NetworkJoined;
use crate::domain::{JoinTicket, SessionDirection};
use crate::ports::{
    BootstrapAttempt, BootstrapRung, ClockPort, DiscoveredPeer, EventPublisherError,
    EventPublisherPort, JoinDiagnostic, JoinOutcome, MembershipCommandError, PeerCachePort,
    PeerDiscoveryPort, PeerTransportPort, RungFailure,
};

/// Reach the network by whatever path is available.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinNetwork {
    /// A ticket to fall back on, if the user pasted one. Validated only if the
    /// two free rungs above it produced nothing.
    pub ticket: Option<JoinTicket>,
}

impl JoinNetwork {
    /// The ordinary launch: cached peers, then the LAN, and no ticket.
    pub const fn unaided() -> Self {
        Self { ticket: None }
    }

    /// A first-ever internet join, with the one pasted ticket D1 costs.
    pub const fn with_ticket(ticket: JoinTicket) -> Self {
        Self {
            ticket: Some(ticket),
        }
    }
}

/// Handles [`JoinNetwork`]: the D1 bootstrap ladder.
///
/// # The ladder
///
/// Cached peers, then the local network, then the pasted ticket — in that
/// order, stopping at the first peer that answers. The order is the cost to
/// the user: a warm cache is free and silent, the LAN is free but short-range,
/// and a ticket is the one rung that needs a human. There is no fourth rung,
/// because every mechanism that makes first-ever contact automatic — hardcoded
/// bootstrap hosts, public rendezvous, DNS seeds — is operator-run
/// infrastructure that S1 forbids.
///
/// One peer is enough. The ladder's job is *first contact*; discovery and
/// gossip supply the rest, and dialling every cached peer on every launch
/// would make a cold start cost proportional to how long this machine has been
/// a member.
///
/// # Never a hang, never a silent failure (AC3)
///
/// Every rung is bounded and every rung reports. A walk in which nothing
/// answers ends at `Isolated` — a normal state, not an error — carrying a
/// [`JoinDiagnostic`] that names each rung and why it produced nothing. That is
/// why this returns `Ok` for a failed join: the only `Err` is the event
/// publisher, the one failure where the peer may have connected while no
/// consumer was told.
///
/// While the walk runs the status reads `Joining`, and the phase is released by
/// a guard, so even an abandoned walk cannot leave a status line stuck on it.
///
/// # Before the ladder
///
/// The transport starts listening and the resulting endpoints are announced —
/// every instance offers discovery to others (AC4), and joining is public by
/// construction (S8). Neither failure stops the walk: a peer that cannot listen
/// can still dial out, it just will not be dialled back, which is stated in the
/// diagnostic rather than left for the user to infer from a peer count that
/// never grows.
#[derive(Clone)]
pub struct JoinNetworkHandler {
    settings: MembershipSettings,
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    transport: Arc<dyn PeerTransportPort + Send + Sync>,
    discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
    cache: Arc<dyn PeerCachePort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    open_session: OpenSessionHandler,
    establish_session: EstablishSessionHandler,
}

impl JoinNetworkHandler {
    pub fn new(
        settings: MembershipSettings,
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
        cache: Arc<dyn PeerCachePort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            open_session: OpenSessionHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&transport),
                Arc::clone(&publisher),
            ),
            establish_session: EstablishSessionHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&publisher),
            ),
            settings,
            state,
            clock,
            transport,
            discovery,
            cache,
            publisher,
        }
    }

    pub fn handle(&self, command: JoinNetwork) -> Result<JoinOutcome, EventPublisherError> {
        let phase = self.state.begin_join();
        let mut diagnostic = JoinDiagnostic::default();

        self.become_reachable(&mut diagnostic);

        let mut connected = None;
        for rung in BootstrapRung::LADDER {
            match self.walk(rung, command.ticket.as_ref())? {
                Ok(peer) => {
                    diagnostic.record(BootstrapAttempt::connected(rung, peer));
                    connected = Some(peer);
                    break;
                }
                Err(failure) => diagnostic.record(BootstrapAttempt::failed(rung, failure)),
            }
        }

        let joined = match connected {
            Some(_) => self.announce_arrival()?,
            None => None,
        };

        // The status must read `Connected(n)` or `Isolated`, never `Joining`:
        // the walk is over by the time the caller reads it.
        drop(phase);

        Ok(JoinOutcome {
            status: self.state.network_status(),
            joined,
            diagnostic,
        })
    }

    /// Starts listening and announces where this peer can be reached.
    ///
    /// Both failures are recorded rather than raised. A peer that cannot listen
    /// still joins over the links it makes itself; a peer that cannot announce
    /// is reachable but undiscoverable. Neither is a reason to refuse to try.
    fn become_reachable(&self, diagnostic: &mut JoinDiagnostic) {
        match self.transport.listen() {
            Ok(endpoints) => {
                if let Err(error) = self.discovery.announce(&endpoints) {
                    diagnostic.announce_failure = Some(error);
                }
            }
            Err(error) => diagnostic.listen_failure = Some(error),
        }
    }

    /// Walks one rung.
    ///
    /// The nested result is deliberate: the outer `Err` is the one failure that
    /// ends the whole join, while the inner `Err` is this rung reporting for
    /// the diagnostic and handing the walk on to the next rung.
    fn walk(
        &self,
        rung: BootstrapRung,
        ticket: Option<&JoinTicket>,
    ) -> Result<Result<PeerId, RungFailure>, EventPublisherError> {
        match rung {
            BootstrapRung::CachedPeers => self.walk_cached_peers(),
            BootstrapRung::LocalNetwork => self.walk_local_network(),
            BootstrapRung::JoinTicket => self.walk_join_ticket(ticket),
        }
    }

    /// Rung (a): the peers this machine knew when it last shut down.
    ///
    /// The rung that makes the ticket a one-time cost — after one successful
    /// join, a machine bootstraps from its own memory and needs neither a LAN
    /// neighbour nor a ticket again.
    fn walk_cached_peers(&self) -> Result<Result<PeerId, RungFailure>, EventPublisherError> {
        let cached = match self.cache.load() {
            Ok(peers) => peers,
            Err(error) => return Ok(Err(error.into())),
        };

        self.dial_candidates(
            cached
                .into_iter()
                .map(|peer| DiscoveredPeer {
                    peer: peer.peer,
                    endpoints: peer.endpoints,
                })
                .collect(),
        )
    }

    /// Rung (b): whatever the discovery mechanism can see by itself — mDNS on
    /// the LAN (AC2), and whatever routing the adapter offers.
    fn walk_local_network(&self) -> Result<Result<PeerId, RungFailure>, EventPublisherError> {
        match self.discovery.observe_peers() {
            Ok(observed) => self.dial_candidates(observed),
            Err(error) => Ok(Err(error.into())),
        }
    }

    /// Rung (c): the out-of-band ticket, and the honest price of internet-wide
    /// first contact with no operator-run infrastructure.
    ///
    /// Validity is checked here — a pure domain rule over the ticket, the
    /// clock, and the protocol this build speaks — so an expired or
    /// wrong-major ticket never reaches the adapter and the user gets the one
    /// answer they can act on: ask the issuer for a fresh one.
    fn walk_join_ticket(
        &self,
        ticket: Option<&JoinTicket>,
    ) -> Result<Result<PeerId, RungFailure>, EventPublisherError> {
        let Some(ticket) = ticket else {
            return Ok(Err(RungFailure::NoCandidates));
        };

        if let Err(error) = ticket.validate(self.clock.now(), self.settings.protocol) {
            return Ok(Err(error.into()));
        }

        match self.discovery.redeem_join_ticket(ticket) {
            Ok(issuer) => self.dial_candidates(vec![issuer]),
            Err(error) => Ok(Err(error.into())),
        }
    }

    /// Tries each candidate in turn and stops at the first that connects.
    ///
    /// The local peer is filtered out before anything is counted: hearing
    /// yourself is routine — a gossiping network echoes every announcement, and
    /// a ticket can be pasted into the machine that minted it — and counting it
    /// as a candidate that failed would make the diagnostic lie.
    fn dial_candidates(
        &self,
        candidates: Vec<DiscoveredPeer>,
    ) -> Result<Result<PeerId, RungFailure>, EventPublisherError> {
        let candidates: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| candidate.peer != self.settings.local_peer)
            .collect();

        if candidates.is_empty() {
            return Ok(Err(RungFailure::NoCandidates));
        }

        let attempted = candidates.len();
        for candidate in candidates {
            let peer = candidate.peer;
            if self.connect(candidate)? {
                return Ok(Ok(peer));
            }
        }

        Ok(Err(RungFailure::Unreachable {
            candidates: attempted,
        }))
    }

    /// Records a candidate and dials it, reporting only whether it connected.
    ///
    /// A roster or transport refusal is this candidate's business — the next
    /// one may work, and the rung's own failure already says how many were
    /// tried. A publisher failure is not: the link may be up with nobody told,
    /// so it ends the join.
    fn connect(&self, candidate: DiscoveredPeer) -> Result<bool, EventPublisherError> {
        let peer = candidate.peer;

        let opened = self.open_session.handle(OpenSession {
            peer,
            direction: SessionDirection::Outbound,
            endpoints: candidate.endpoints,
        });
        if !Self::candidate_survived(opened)? {
            return Ok(false);
        }

        let established = self.establish_session.handle(EstablishSession { peer });
        Self::candidate_survived(established)
    }

    fn candidate_survived<T>(
        outcome: Result<T, MembershipCommandError>,
    ) -> Result<bool, EventPublisherError> {
        match outcome {
            Ok(_) => Ok(true),
            Err(MembershipCommandError::Publisher(error)) => Err(error),
            Err(MembershipCommandError::Roster(_) | MembershipCommandError::Transport(_)) => {
                Ok(false)
            }
        }
    }

    /// Publishes `NetworkJoined`, once, for a walk that connected something new.
    ///
    /// A re-join that connects nobody new announces nothing: the peer count is
    /// unchanged and telling consumers it arrived a second time would be an
    /// event with no change behind it.
    fn announce_arrival(&self) -> Result<Option<NetworkJoined>, EventPublisherError> {
        let count = self.state.read(|roster| roster.established_session_count());
        let Some(connected_peers) = NonZeroUsize::new(count) else {
            return Ok(None);
        };

        let event = NetworkJoined {
            at: self.clock.now(),
            connected_peers,
        };
        self.publisher.publish(event.into())?;
        Ok(Some(event))
    }
}
