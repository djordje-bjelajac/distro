use std::sync::Arc;

use shared_types::PeerId;

use crate::application::commands::{
    EstablishSession, EstablishSessionHandler, ForgetKnownPeers, ForgetKnownPeersHandler,
    JoinNetwork, JoinNetworkHandler, LeaveNetwork, LeaveNetworkHandler, OpenSession,
    OpenSessionHandler,
};
use crate::application::{MembershipSettings, MembershipState};
use crate::domain::{JoinTicket, SessionDirection, SessionOutcome};
use crate::ports::{
    ClockPort, EventPublisherError, EventPublisherPort, ForgetPeersError, ForgetPeersOutcome,
    JoinNetworkPort, JoinOutcome, LeaveOutcome, MembershipCommandError, PeerCachePort,
    PeerDiscoveryPort, PeerTransportPort,
};

/// The decision half of this context's inbound surface: one
/// [`JoinNetworkPort`] implementation over the handlers a person or a startup
/// step drives.
///
/// It holds handlers rather than reimplementing them, so each use case keeps
/// its own file and its own tests; this type adds only the translation from
/// the port's domain-typed arguments to the imperative command DTOs.
///
/// [`connect_to_peer`](JoinNetworkPort::connect_to_peer) is the one composite:
/// a dial that answers has already completed its authenticated handshake, so
/// opening and establishing happen together. The two commands stay separate
/// because the inbound path genuinely finishes its handshake later.
#[derive(Clone)]
pub struct JoinNetworkService {
    join_network: JoinNetworkHandler,
    leave_network: LeaveNetworkHandler,
    forget_known_peers: ForgetKnownPeersHandler,
    open_session: OpenSessionHandler,
    establish_session: EstablishSessionHandler,
}

impl JoinNetworkService {
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
            join_network: JoinNetworkHandler::new(
                settings,
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&transport),
                discovery,
                Arc::clone(&cache),
                Arc::clone(&publisher),
            ),
            leave_network: LeaveNetworkHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&transport),
                Arc::clone(&cache),
                Arc::clone(&publisher),
            ),
            // Holds its own leave handler rather than borrowing the one above:
            // forgetting *is* a leave followed by two more steps, and the
            // duplication is one `Arc` clone against a shared-mutable-handler
            // seam nobody would enjoy debugging.
            forget_known_peers: ForgetKnownPeersHandler::new(
                Arc::clone(&state),
                Arc::clone(&cache),
                LeaveNetworkHandler::new(
                    Arc::clone(&state),
                    Arc::clone(&clock),
                    Arc::clone(&transport),
                    cache,
                    Arc::clone(&publisher),
                ),
            ),
            open_session: OpenSessionHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                transport,
                Arc::clone(&publisher),
            ),
            establish_session: EstablishSessionHandler::new(state, clock, publisher),
        }
    }
}

impl JoinNetworkPort for JoinNetworkService {
    fn join_network(&self, ticket: Option<JoinTicket>) -> Result<JoinOutcome, EventPublisherError> {
        self.join_network.handle(JoinNetwork { ticket })
    }

    fn leave_network(&self) -> Result<LeaveOutcome, EventPublisherError> {
        self.leave_network.handle(LeaveNetwork)
    }

    fn connect_to_peer(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.open_session.handle(OpenSession {
            peer,
            direction: SessionDirection::Outbound,
            endpoints: Vec::new(),
        })?;

        self.establish_session.handle(EstablishSession { peer })
    }

    fn forget_known_peers(&self) -> Result<ForgetPeersOutcome, ForgetPeersError> {
        self.forget_known_peers.handle(ForgetKnownPeers)
    }
}
