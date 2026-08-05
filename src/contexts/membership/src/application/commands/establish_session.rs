use std::sync::Arc;

use shared_types::PeerId;

use crate::application::{MembershipState, SessionOutcomeDispatcher};
use crate::domain::SessionOutcome;
use crate::ports::{ClockPort, EventPublisherPort, MembershipCommandError};

/// Record that the authenticated handshake with a peer completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EstablishSession {
    pub peer: PeerId,
}

/// Handles [`EstablishSession`]: the single point at which this context calls
/// a peer reachable.
///
/// `PeerConnected` is published here and nowhere else, which is what makes it
/// meaningful to `messaging` (D10): the event says a link exists that bytes can
/// actually cross, not that a dial was attempted. Establishing twice is
/// rejected by the domain rather than ignored, so the event cannot be published
/// twice for one link.
///
/// The instant is recorded as evidence of life: a completed handshake is proof
/// the remote acted just now, which is exactly what presence is derived from.
#[derive(Clone)]
pub struct EstablishSessionHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    dispatcher: SessionOutcomeDispatcher,
}

impl EstablishSessionHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            state,
            clock,
            dispatcher: SessionOutcomeDispatcher::new(publisher),
        }
    }

    pub fn handle(
        &self,
        command: EstablishSession,
    ) -> Result<SessionOutcome, MembershipCommandError> {
        let now = self.clock.now();

        let outcome = self
            .state
            .modify(|roster| roster.establish_session(command.peer, now))?;

        self.dispatcher.publish(&outcome)?;
        Ok(outcome)
    }
}
