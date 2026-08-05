use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::application::queries::{
    GetNetworkStatus, GetNetworkStatusHandler, ListKnownPeers, ListKnownPeersHandler,
    ListOnlinePeers, ListOnlinePeersHandler,
};
use crate::domain::{LivenessWindows, NetworkStatus};
use crate::ports::{ClockPort, KnownPeerView, MembershipQueryPort};

/// The read half of this context's inbound surface: one
/// [`MembershipQueryPort`] implementation over the three query handlers.
///
/// It holds handlers rather than reimplementing them, so each read model keeps
/// its own file and its own tests; this type adds only the translation from
/// the port's arguments to the query DTOs and contains no decision of its own.
///
/// Wired over the same [`MembershipState`] as the command services, so a
/// session the network just reported is visible on the very next redraw. No
/// method here writes, and none can: every handler behind it takes the roster
/// lock through the read accessor only.
#[derive(Clone)]
pub struct MembershipQueryService {
    known_peers: ListKnownPeersHandler,
    online_peers: ListOnlinePeersHandler,
    network_status: GetNetworkStatusHandler,
}

impl MembershipQueryService {
    /// Wires the query side over the shared state, the clock, and the liveness
    /// windows presence is derived against.
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        windows: LivenessWindows,
    ) -> Self {
        Self {
            known_peers: ListKnownPeersHandler::new(
                Arc::clone(&state),
                Arc::clone(&clock),
                windows,
            ),
            online_peers: ListOnlinePeersHandler::new(Arc::clone(&state), clock, windows),
            network_status: GetNetworkStatusHandler::new(state),
        }
    }
}

impl MembershipQueryPort for MembershipQueryService {
    fn known_peers(&self) -> Vec<KnownPeerView> {
        self.known_peers.handle(ListKnownPeers)
    }

    fn online_peers(&self) -> Vec<PeerId> {
        self.online_peers.handle(ListOnlinePeers)
    }

    fn network_status(&self) -> NetworkStatus {
        self.network_status.handle(GetNetworkStatus)
    }
}
