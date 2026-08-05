use std::sync::Arc;

use crate::application::MembershipState;
use crate::domain::LivenessWindows;
use crate::ports::{ClockPort, KnownPeerView};

/// Ask for every peer this instance knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListKnownPeers;

/// Handles [`ListKnownPeers`]: the roster pane's read model.
///
/// Reads the clock once and derives every peer's presence against that single
/// instant (invariant 7). One reading rather than one per peer is not a
/// micro-optimisation: two peers classified against two different `now`s could
/// disagree about an age they share, and a redraw would show a boundary
/// flickering that never happened.
///
/// Writes nothing — not the roster, not a stored presence, not a "last
/// rendered" marker. `Presence` is derived on the way out, so rendering the
/// roster a thousand times leaves it byte-identical.
///
/// The order is the roster's own `PeerId` order, which makes a redraw and a
/// recorded trace deterministic (S5).
#[derive(Clone)]
pub struct ListKnownPeersHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    windows: LivenessWindows,
}

impl ListKnownPeersHandler {
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

    pub fn handle(&self, _query: ListKnownPeers) -> Vec<KnownPeerView> {
        let now = self.clock.now();

        self.state.read(|roster| {
            roster
                .known_peers()
                .map(|entry| KnownPeerView::of(entry, now, self.windows))
                .collect()
        })
    }
}
