use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::ports::{ClockPort, MembershipCommandError};

/// Record that a peer produced evidence of life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordPeerHeartbeat {
    /// The peer that was heard from.
    pub peer: PeerId,
}

/// Handles [`RecordPeerHeartbeat`]: the input side of every presence
/// derivation (invariant 7).
///
/// Anything a peer sends is evidence — a keep-alive, a message, a re-dial. The
/// handler only stamps the instant; whether that makes the peer `Online`,
/// `Stale`, or `Offline` is derived at read time from the age of this stamp,
/// which is why nothing here computes or stores a presence.
///
/// Publishes nothing. A peer still being alive is not news; the transition
/// *out* of silence has no consumer, and the transition *into* it is
/// `ExpirePresence`'s to announce.
///
/// Sessions are untouched: traffic on a link does not establish it, and a peer
/// with no session at all can be perfectly alive.
#[derive(Clone)]
pub struct RecordPeerHeartbeatHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
}

impl RecordPeerHeartbeatHandler {
    pub fn new(state: Arc<MembershipState>, clock: Arc<dyn ClockPort + Send + Sync>) -> Self {
        Self { state, clock }
    }

    pub fn handle(&self, command: RecordPeerHeartbeat) -> Result<(), MembershipCommandError> {
        let now = self.clock.now();

        self.state
            .modify(|roster| roster.record_heartbeat(command.peer, now))?;

        Ok(())
    }
}
