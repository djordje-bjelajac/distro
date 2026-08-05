use std::sync::Arc;

use shared_types::{PeerDisconnected, PeerId};

use crate::application::MembershipState;
use crate::application::commands::{CloseSession, CloseSessionHandler, SessionCloseCause};
use crate::domain::events::NetworkLeft;
use crate::domain::{KnownPeer, Session};
use crate::ports::{
    CachedPeer, ClockPort, EventPublisherError, EventPublisherPort, LeaveOutcome,
    MembershipCommandError, PeerCachePort, PeerTransportPort,
};

/// Leave the network deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaveNetwork;

/// Handles [`LeaveNetwork`]: close every session, keep the addresses, say so.
///
/// # Order, and why it is the order
///
/// 1. Close every live session, announcing each established one.
/// 2. Save the roster's peers for the next launch's first bootstrap rung (D1).
/// 3. Announce the departure.
///
/// `NetworkLeft` goes last so no consumer sees the network left while it still
/// believes a link is live. The cache is written before it because the save is
/// the whole point of leaving cleanly: it is what keeps the join ticket a
/// one-time cost on this machine.
///
/// # This is a decision, not an observation
///
/// Losing the last session to a network failure is *not* this event — that is a
/// `PeerDisconnected` and a return to `Isolated`. Conflating the two would let
/// a UI report a departure the user never asked for.
///
/// A cache that cannot be written is reported in the outcome rather than
/// raised: the departure happened, and the cost is a colder start next time. It
/// is still reported, because a machine that silently stops warm-starting ends
/// up needing a ticket again with no explanation.
#[derive(Clone)]
pub struct LeaveNetworkHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    cache: Arc<dyn PeerCachePort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    close_session: CloseSessionHandler,
}

impl LeaveNetworkHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        cache: Arc<dyn PeerCachePort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            close_session: CloseSessionHandler::new(
                Arc::clone(&state),
                transport,
                Arc::clone(&publisher),
            ),
            state,
            clock,
            cache,
            publisher,
        }
    }

    pub fn handle(&self, _command: LeaveNetwork) -> Result<LeaveOutcome, EventPublisherError> {
        let at = self.clock.now();
        let disconnected = self.close_every_session()?;

        let peers: Vec<CachedPeer> = self
            .state
            .read(|roster| roster.known_peers().map(CachedPeer::of).collect());
        let cache_failure = self.cache.save(&peers).err();

        let left = NetworkLeft { at };
        self.publisher.publish(left.into())?;

        Ok(LeaveOutcome {
            left,
            disconnected,
            cached_peers: peers.len(),
            cache_failure,
        })
    }

    /// Closes every live session in `PeerId` order and collects the departures
    /// that were actually announced.
    fn close_every_session(&self) -> Result<Vec<PeerDisconnected>, EventPublisherError> {
        let live = self.state.read(|roster| {
            roster
                .known_peers()
                .filter(|entry| entry.session().is_some_and(Session::is_live))
                .map(KnownPeer::peer)
                .collect::<Vec<PeerId>>()
        });

        let mut disconnected = Vec::new();
        for peer in live {
            let outcome = self.close_session.handle(CloseSession {
                peer,
                cause: SessionCloseCause::LocalDecision,
            });

            match outcome {
                Ok(outcome) => disconnected.extend(outcome.disconnected),
                Err(MembershipCommandError::Publisher(error)) => return Err(error),
                // The roster refused, which here can only mean the session
                // ended between the read and the close. Leaving is exactly
                // when that race is least worth caring about.
                Err(MembershipCommandError::Roster(_) | MembershipCommandError::Transport(_)) => {}
            }
        }

        Ok(disconnected)
    }
}
