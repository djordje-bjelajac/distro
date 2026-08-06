use std::sync::Arc;

use crate::application::MembershipState;
use crate::domain::LivenessWindows;
use crate::ports::{ClockPort, KnownPeerView, NetworkView};

/// Ask for the whole network picture at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetNetworkView;

/// Handles [`GetNetworkView`]: the status line and the roster rows from one
/// clock reading, one roster traversal, and one classification (canvas D5).
///
/// # What the single snapshot is and is not
///
/// It is *not* the fix for the observed screen. `connected (2 peers)` above a
/// roster of `offline` rows was a semantic contradiction — the count came from
/// the session predicate and the rows from the age of the evidence — and it
/// would have survived any number of atomic reads. The fix is that
/// [`NetworkView`] derives its count from the standings of the very rows it
/// carries, so the two cannot be stated independently.
///
/// The snapshot is what makes that derivation *meaningful*. Deriving one
/// classification from rows fetched at instant A and a count fetched at instant
/// B would put the coherence guarantee back in the caller's hands, which is
/// where it failed.
///
/// # One clock reading
///
/// Every row's presence is derived against the same `now`. One reading per peer
/// is not a micro-optimisation to skip: two peers holding identical evidence
/// classified against two different instants could land on opposite sides of a
/// window boundary, and a redraw would show a transition that never happened.
///
/// # One roster acquisition
///
/// The join bit is read first and from an atomic, not from the roster (see
/// [`MembershipState::is_joining`]), so the whole view costs one lock. Reading
/// it first also preserves the older path's precedence: a re-join over live
/// sessions is still a join, and reporting `Connected(n)` while the ladder is
/// still walking would answer a question the caller did not ask.
///
/// Writes nothing, like every handler in this module: presence is derived on
/// the way out, so rendering the network a thousand times leaves the roster
/// byte-identical.
#[derive(Clone)]
pub struct GetNetworkViewHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    windows: LivenessWindows,
}

impl GetNetworkViewHandler {
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

    pub fn handle(&self, _query: GetNetworkView) -> NetworkView {
        let now = self.clock.now();
        let joining = self.state.is_joining();

        let peers: Vec<KnownPeerView> = self.state.read(|roster| {
            roster
                .known_peers()
                .map(|entry| KnownPeerView::of(entry, now, self.windows))
                .collect()
        });

        if joining {
            NetworkView::joining(peers)
        } else {
            NetworkView::of(peers)
        }
    }
}
