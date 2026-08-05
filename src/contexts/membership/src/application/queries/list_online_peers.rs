use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::domain::{LivenessWindows, Presence};
use crate::ports::ClockPort;

/// Ask which peers this instance currently believes are alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListOnlinePeers;

/// Handles [`ListOnlinePeers`]: the peers whose evidence of life is younger
/// than the online window, as of one clock reading.
///
/// # Online is not connected
///
/// A peer that announced itself a second ago is `Online` with no session at
/// all, and a peer holding an established session goes `Stale` and then
/// `Offline` if it stops speaking — the session does not keep it alive,
/// because a link staying open is not evidence that anything is behind it.
/// Callers asking "can I reach it right now" want the `is_connected` flag on
/// [`ListKnownPeers`](crate::application::queries::ListKnownPeers); callers
/// asking "who is around" want this.
///
/// `Stale` peers are excluded deliberately. The middle band exists so the view
/// can admit it does not know (invariant 7), and folding it into `Online`
/// would spend that honesty on a longer list.
#[derive(Clone)]
pub struct ListOnlinePeersHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    windows: LivenessWindows,
}

impl ListOnlinePeersHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        windows: LivenessWindows,
    ) -> Self {
        Self {
            state,
            clock,
            windows,
        }
    }

    pub fn handle(&self, _query: ListOnlinePeers) -> Vec<PeerId> {
        let now = self.clock.now();

        self.state.read(|roster| {
            roster
                .known_peers()
                .filter(|entry| matches!(entry.presence(now, self.windows), Presence::Online))
                .map(crate::domain::KnownPeer::peer)
                .collect()
        })
    }
}
