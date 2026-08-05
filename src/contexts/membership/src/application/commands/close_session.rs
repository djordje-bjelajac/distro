use std::sync::Arc;

use shared_types::PeerId;

use crate::application::commands::SessionCloseCause;
use crate::application::{MembershipState, SessionOutcomeDispatcher};
use crate::domain::SessionOutcome;
use crate::ports::{EventPublisherPort, MembershipCommandError, PeerTransportPort};

/// End the session with a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseSession {
    pub peer: PeerId,
    /// Who ended it, which decides whether the transport is asked to close the
    /// link or is the one that reported it gone.
    pub cause: SessionCloseCause,
}

/// Handles [`CloseSession`]: the session ends, the peer stays known.
///
/// # What is not forgotten
///
/// The roster entry survives. Its addresses are the next launch's first
/// bootstrap rung (D1), and dropping them on every disconnect would make a
/// peer that reconnects hourly permanently cold.
///
/// # What is not announced
///
/// `PeerDisconnected` only if the session had established. A session that died
/// while connecting was never announced as connected, and an unmatched
/// disconnect would make `messaging` fail directs for a peer it never
/// considered reachable (D10).
///
/// # Why a transport failure does not fail the close
///
/// The link is being abandoned either way. `NoSuchSession` is the ordinary
/// race — the remote closed first — and `Unavailable` means the transport is
/// down, which is when leaving is most likely to be what the user wants. The
/// roster is the authority on what this peer believes, so it closes and
/// announces regardless.
///
/// No clock reading: a close is not evidence of life. A locally initiated one
/// says nothing about the remote at all.
#[derive(Clone)]
pub struct CloseSessionHandler {
    state: Arc<MembershipState>,
    transport: Arc<dyn PeerTransportPort + Send + Sync>,
    dispatcher: SessionOutcomeDispatcher,
}

impl CloseSessionHandler {
    pub fn new(
        state: Arc<MembershipState>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            state,
            transport,
            dispatcher: SessionOutcomeDispatcher::new(publisher),
        }
    }

    pub fn handle(&self, command: CloseSession) -> Result<SessionOutcome, MembershipCommandError> {
        let outcome = self
            .state
            .modify(|roster| roster.close_session(command.peer))?;

        if command.cause.closes_the_transport_link() {
            let _ = self.transport.close_session(command.peer);
        }

        self.dispatcher.publish(&outcome)?;
        Ok(outcome)
    }
}
