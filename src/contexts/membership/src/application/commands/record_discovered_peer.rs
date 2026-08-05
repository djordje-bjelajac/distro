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
/// later one merges addresses and refreshes evidence of life in silence. A
/// consumer drowned in events that carry no news would learn to ignore them.
///
/// Two rejections that look like errors are not:
///
/// * The local peer's own announcement comes back from any gossiping network,
///   and its own join ticket can be pasted into the machine that minted it.
///   That is [`DiscoveryOutcome::OwnAnnouncement`] — invariant 2 is enforced,
///   without an adapter having to treat the most routine event on the wire as
///   a fault.
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
        let now = self.clock.now();
        let DiscoveredPeer { peer, endpoints } = command.discovered;

        let recorded = self
            .state
            .modify(|roster| roster.record_discovery(peer, endpoints, now));

        match recorded {
            Ok(Some(event)) => {
                self.publisher.publish(event.into())?;
                Ok(DiscoveryOutcome::Recorded(event))
            }
            Ok(None) => Ok(DiscoveryOutcome::Refreshed),
            Err(PeerRosterError::SelfConnection) => Ok(DiscoveryOutcome::OwnAnnouncement),
            Err(error) => Err(error.into()),
        }
    }
}
