use std::sync::Arc;

use crate::application::MembershipState;
use crate::domain::PeerRosterError;
use crate::ports::{
    ClockPort, DiscoveredPeer, DiscoveryOutcome, EventPublisherPort, MembershipCommandError,
};

/// Record that a discovery mechanism saw a peer at the addresses it claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDiscoveredPeer {
    /// What the mechanism reported. A claim by whoever made it — the identity
    /// is only proven at the session handshake.
    pub discovered: DiscoveredPeer,
}

/// Handles [`RecordDiscoveredPeer`]: the way a peer becomes dialable.
///
/// Discovery is loud. The same peer arrives from mDNS, from routing, and from
/// other peers' announcements, over and over, so only the *first* sighting
/// produces [`PeerDiscovered`](crate::domain::events::PeerDiscovered); every
/// later one merges addresses in silence. A consumer drowned in events that
/// carry no news would learn to ignore them.
///
/// # The clock reading here dates a record, not a peer
///
/// This handler reads the clock, and what the instant becomes is the roster
/// entry's `recorded_at` — when *we wrote the sighting down*. It is not
/// evidence and never reaches presence. A sighting is something a third party
/// said (invariant 2): mDNS records are spoofable by any host on the link and
/// DHT records are written by whoever felt like writing them, so treating one
/// as evidence let a hostile host keep arbitrary victims `Online` in every
/// roster that learned the record, refreshed on every re-announcement (canvas
/// D3, safeguard S2).
///
/// Three outcomes that look like errors are not:
///
/// * The local peer's own announcement comes back from any gossiping network,
///   and its own join ticket can be pasted into the machine that minted it.
///   That is [`DiscoveryOutcome::OwnAnnouncement`] — invariant 2 is enforced,
///   without an adapter having to treat the most routine event on the wire as
///   a fault.
/// * A full roster refuses a new identity, which is what the cap is for under
///   exactly the load that fills it. That is [`DiscoveryOutcome::RosterFull`];
///   the reasoning for making it a state is on that type.
/// * A sighting with no address is genuinely useless, and *is* an error: an
///   adapter reporting a peer with nowhere to reach it has reported nothing.
#[derive(Clone)]
pub struct RecordDiscoveredPeerHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
}

impl RecordDiscoveredPeerHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self {
            state,
            clock,
            publisher,
        }
    }

    pub fn handle(
        &self,
        command: RecordDiscoveredPeer,
    ) -> Result<DiscoveryOutcome, MembershipCommandError> {
        let recorded_at = self.clock.now();
        let DiscoveredPeer { peer, endpoints } = command.discovered;

        let recorded = self
            .state
            .modify(|roster| roster.record_discovery(peer, endpoints, recorded_at));

        match recorded {
            Ok(Some(event)) => {
                self.publisher.publish(event.into())?;
                Ok(DiscoveryOutcome::Recorded(event))
            }
            Ok(None) => Ok(DiscoveryOutcome::Refreshed),
            Err(PeerRosterError::SelfConnection) => Ok(DiscoveryOutcome::OwnAnnouncement),
            Err(PeerRosterError::RosterFull) => Ok(DiscoveryOutcome::RosterFull),
            Err(error) => Err(error.into()),
        }
    }
}
