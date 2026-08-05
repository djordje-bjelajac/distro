use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::application::commands::{
    CloseSession, CloseSessionHandler, EstablishSession, EstablishSessionHandler, ExpirePresence,
    ExpirePresenceHandler, OpenSession, OpenSessionHandler, RecordDiscoveredPeer,
    RecordDiscoveredPeerHandler, RecordPeerHeartbeat, RecordPeerHeartbeatHandler,
    SessionCloseCause,
};
use crate::domain::events::PeerPresenceExpired;
use crate::domain::{Endpoint, LivenessWindows, SessionDirection, SessionOutcome};
use crate::ports::{
    ClockPort, DiscoveredPeer, DiscoveryOutcome, EventPublisherError, EventPublisherPort,
    InboundSessionPort, MembershipCommandError, PeerTransportPort,
};

/// The report half of this context's inbound surface: one
/// [`InboundSessionPort`] implementation over the handlers the network runtime
/// drives (S3).
///
/// Every method here is a *report* — a peer announced itself, a remote dialled
/// in, a handshake finished, a link died, nobody has spoken in a while — and
/// the direction of every session it names is `Inbound` by construction. An
/// outbound dial is a decision and belongs to
/// [`JoinNetworkPort`](crate::ports::JoinNetworkPort), which is why no argument
/// here carries a direction for a transport to get wrong.
#[derive(Clone)]
pub struct InboundSessionService {
    record_discovered_peer: RecordDiscoveredPeerHandler,
    open_session: OpenSessionHandler,
    establish_session: EstablishSessionHandler,
    close_session: CloseSessionHandler,
    record_peer_heartbeat: RecordPeerHeartbeatHandler,
    expire_presence: ExpirePresenceHandler,
}

impl InboundSessionService {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
        windows: LivenessWindows,
    ) -> Self {
        Self {
            record_discovered_peer: RecordDiscoveredPeerHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&publisher),
            ),
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
            close_session: CloseSessionHandler::new(
                Arc::clone(&state),
                transport,
                Arc::clone(&publisher),
            ),
            record_peer_heartbeat: RecordPeerHeartbeatHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
            ),
            expire_presence: ExpirePresenceHandler::new(state, clock, publisher, windows),
        }
    }
}

impl InboundSessionPort for InboundSessionService {
    fn peer_observed(
        &self,
        discovered: DiscoveredPeer,
    ) -> Result<DiscoveryOutcome, MembershipCommandError> {
        self.record_discovered_peer
            .handle(RecordDiscoveredPeer { discovered })
    }

    fn session_opened(
        &self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
    ) -> Result<SessionOutcome, MembershipCommandError> {
        self.open_session.handle(OpenSession {
            peer,
            direction: SessionDirection::Inbound,
            endpoints,
        })
    }

    fn session_established(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.establish_session.handle(EstablishSession { peer })
    }

    fn session_closed(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.close_session.handle(CloseSession {
            peer,
            cause: SessionCloseCause::TransportReported,
        })
    }

    fn peer_heartbeat(&self, peer: PeerId) -> Result<(), MembershipCommandError> {
        self.record_peer_heartbeat
            .handle(RecordPeerHeartbeat { peer })
    }

    fn expire_presence(&self) -> Result<Vec<PeerPresenceExpired>, EventPublisherError> {
        self.expire_presence.handle(ExpirePresence)
    }
}
