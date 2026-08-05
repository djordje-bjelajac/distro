use std::sync::Arc;

use crate::application::MembershipState;
use crate::domain::NetworkStatus;

/// Ask how connected this instance currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetNetworkStatus;

/// Handles [`GetNetworkStatus`]: the one line a UI shows about the network
/// (canvas §2.2, S7).
///
/// Three answers, and each is the truth about a different thing: `Joining`
/// means a bootstrap ladder is in flight, `Connected(n)` counts *established*
/// sessions — a session still handshaking is not yet reachability — and
/// `Isolated` means neither, which is a normal state for a fresh install with
/// no cached peers, no LAN neighbour, and no ticket (D1). Reporting isolation
/// as a failure is exactly the lie S7 asks the UI not to tell.
///
/// This handler takes no clock: connectivity is a fact about sessions, not an
/// age. Presence is the time-dependent question, and it is asked elsewhere.
#[derive(Clone)]
pub struct GetNetworkStatusHandler {
    state: Arc<MembershipState>,
}

impl GetNetworkStatusHandler {
    pub const fn new(state: Arc<MembershipState>) -> Self {
        Self { state }
    }

    pub fn handle(&self, _query: GetNetworkStatus) -> NetworkStatus {
        self.state.network_status()
    }
}
