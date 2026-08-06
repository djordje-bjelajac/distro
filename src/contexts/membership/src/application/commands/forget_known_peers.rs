use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::commands::{LeaveNetwork, LeaveNetworkHandler};
use crate::ports::{ForgetPeersError, ForgetPeersOutcome, PeerCachePort};

/// Forget every peer this instance knows, so the next launch is a cold start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForgetKnownPeers;

/// Handles [`ForgetKnownPeers`]: leave, then forget, then write nothing down.
///
/// # Order, and why it is the whole operation
///
/// 1. **Leave.** Close every live session, announce every established
///    departure — [`LeaveNetworkHandler`] verbatim, not a copy of it.
/// 2. **Forget.** Empty the roster.
/// 3. **Save nothing.** Replace the cache with an empty set.
///
/// Reverse the first two and the feature stops working. Sessions live *inside*
/// roster entries, so a roster emptied first leaves nothing for a leave to
/// close: the transport keeps every link, the next inbound frame recreates the
/// entry through discovery, and the peer the user asked to forget is back
/// within seconds. That is why this handler owns a `LeaveNetworkHandler`
/// rather than reaching for [`PeerRoster::forget_all`] directly.
///
/// [`PeerRoster::forget_all`]: crate::domain::PeerRoster::forget_all
///
/// # The cache is written twice, on purpose
///
/// Step 1 saves the populated cache, because that is what leaving does and
/// this handler does not get to pick a different leave. Step 3 then overwrites
/// it with nothing. The intermediate write is the price of having one code
/// path for "close everything" instead of a second, nearly identical one, and
/// it is a few hundred bytes to a local file. What must not happen — and what
/// the tests pin — is the reverse: an empty write followed by a populated one,
/// which is exactly what a quit does to a naive implementation that only
/// emptied the file.
///
/// # What this refuses
///
/// A join in flight. The ladder reads the cache on its own thread and dials
/// what it finds; a forget landing between that read and the dial produces a
/// join from peers the user has just erased. Refusal is typed and reported,
/// never silent.
///
/// # What this does not touch
///
/// Trust records, the keypair, the outbound sequence counter. Forgetting a
/// peer is not a reason to unblock it, to change identity, or to go mute, and
/// no line here reaches any of the three.
#[derive(Clone)]
pub struct ForgetKnownPeersHandler {
    state: Arc<MembershipState>,
    cache: Arc<dyn PeerCachePort + Send + Sync>,
    leave_network: LeaveNetworkHandler,
}

impl ForgetKnownPeersHandler {
    pub fn new(
        state: Arc<MembershipState>,
        cache: Arc<dyn PeerCachePort + Send + Sync>,
        leave_network: LeaveNetworkHandler,
    ) -> Self {
        Self {
            state,
            cache,
            leave_network,
        }
    }

    pub fn handle(
        &self,
        _command: ForgetKnownPeers,
    ) -> Result<ForgetPeersOutcome, ForgetPeersError> {
        if self.state.is_joining() {
            return Err(ForgetPeersError::JoinInFlight);
        }

        let left = self.leave_network.handle(LeaveNetwork)?;
        let forgotten = self.state.modify(|roster| roster.forget_all());

        // Last, and after the roster is empty: this write is the one that has
        // to survive, and a save that raced the leave's own could otherwise
        // land first.
        let cache_failure = self.cache.save(&[]).err();

        Ok(ForgetPeersOutcome {
            forgotten,
            disconnected: left.disconnected,
            cache_failure,
        })
    }
}
