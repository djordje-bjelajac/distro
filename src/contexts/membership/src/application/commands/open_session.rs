use std::sync::Arc;

use shared_types::PeerId;

use crate::application::commands::{RecordDiscoveredPeer, RecordDiscoveredPeerHandler};
use crate::application::{MembershipState, SessionOutcomeDispatcher};
use crate::domain::{Endpoint, PeerRosterError, SessionDirection, SessionOutcome};
use crate::ports::{
    ClockPort, DiscoveredPeer, EventPublisherPort, MembershipCommandError, PeerTransportPort,
};

/// Start a session with a peer, in one direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSession {
    pub peer: PeerId,
    /// Who dialled. `Outbound` makes this handler dial; `Inbound` means the
    /// link already exists because the remote made it.
    pub direction: SessionDirection,
    /// Addresses to record for the peer first, if any. An inbound link carries
    /// the address it came from; an outbound dial uses what the roster already
    /// holds and leaves this empty.
    pub endpoints: Vec<Endpoint>,
}

/// Handles [`OpenSession`]: the transition into `Connecting`, in either
/// direction.
///
/// # Discovery comes first, on purpose
///
/// A peer that redeemed *our* join ticket dials us before we have ever heard of
/// it, so an inbound open has to be able to enter the peer in the roster from
/// the address the link came from. Without that, the ticket rung of D1 would
/// work in one direction only.
///
/// A full roster refuses that entry as a state rather than an error
/// ([`DiscoveryOutcome::RosterFull`](crate::ports::DiscoveryOutcome::RosterFull)),
/// so the session transition below is what reports the consequence: with no
/// entry to attach to, it answers `UnknownPeer`. That is accurate — the peer is
/// not in the roster — and it is the refusal every caller already handles as
/// "this one did not work", the bootstrap ladder included.
///
/// # A dial is a handshake
///
/// `PeerTransportPort::dial` reports `HandshakeFailed` separately from
/// "answered", so a successful dial means the authenticated link is up. This
/// handler still stops at `Connecting`:
/// [`EstablishSession`](crate::application::commands::EstablishSession)
/// publishes `PeerConnected`, and keeping the two apart is what lets the
/// inbound path — where the handshake genuinely completes later — use exactly
/// the same commands.
///
/// # Simultaneous connect
///
/// A peer dialling us while we dial it is the *normal* case in a symmetric
/// network (invariant 3). The roster collapses it deterministically and names
/// the superseded direction; this handler passes that back untouched, because
/// the transport closes by peer and only the caller holding the two link
/// handles can close the right one.
///
/// # Why the dial happens before the roster transition
///
/// The reverse order is defensible and was rejected. Opening the roster session
/// first would mean the collapse rule fires while the dial is still in flight,
/// and a collapse that supersedes an *established* session tears down a working
/// link — which would then have been spent on a dial that may still fail. Losing
/// a live link to a dial that never lands is worse than the cost of this order,
/// which is that a roster refusal *after* a successful dial leaves the transport
/// holding a link the roster does not describe. That refusal can only be
/// `SessionAlreadyOpen` (every other rejection is decided before the dial), it
/// means the caller already had a session it lost track of, and a real transport
/// answers a second dial to a connected peer with the link it already has.
#[derive(Clone)]
pub struct OpenSessionHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    transport: Arc<dyn PeerTransportPort + Send + Sync>,
    dispatcher: SessionOutcomeDispatcher,
    record_discovered_peer: RecordDiscoveredPeerHandler,
}

impl OpenSessionHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        transport: Arc<dyn PeerTransportPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            record_discovered_peer: RecordDiscoveredPeerHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                Arc::clone(&publisher),
            ),
            dispatcher: SessionOutcomeDispatcher::new(publisher),
            state,
            clock,
            transport,
        }
    }

    pub fn handle(&self, command: OpenSession) -> Result<SessionOutcome, MembershipCommandError> {
        let OpenSession {
            peer,
            direction,
            endpoints,
        } = command;

        if !endpoints.is_empty() {
            self.record_discovered_peer.handle(RecordDiscoveredPeer {
                discovered: DiscoveredPeer { peer, endpoints },
            })?;
        }

        if matches!(direction, SessionDirection::Outbound) {
            self.dial(peer)?;
        }

        let now = self.clock.now();
        let outcome = self
            .state
            .modify(|roster| roster.open_session(peer, direction, now))?;

        self.dispatcher.publish(&outcome)?;
        Ok(outcome)
    }

    /// Dials `peer` at every address the roster holds for it.
    ///
    /// The peer must be known: a dial needs an address, and an address is what
    /// discovery is for. `UnknownPeer` here is the honest answer rather than a
    /// silently skipped step.
    fn dial(&self, peer: PeerId) -> Result<(), MembershipCommandError> {
        let endpoints = self
            .state
            .read(|roster| roster.peer(&peer).map(|entry| entry.endpoints().to_vec()))
            .ok_or(PeerRosterError::UnknownPeer)?;

        self.transport.dial(peer, &endpoints)?;
        Ok(())
    }
}
