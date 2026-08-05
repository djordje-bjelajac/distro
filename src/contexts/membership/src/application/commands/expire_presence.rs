use std::sync::Arc;

use crate::application::MembershipState;
use crate::domain::LivenessWindows;
use crate::domain::events::PeerPresenceExpired;
use crate::ports::{ClockPort, EventPublisherError, EventPublisherPort};

/// Re-derive every peer's presence and announce those that have newly fallen
/// silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExpirePresence;

/// Handles [`ExpirePresence`]: the clock-driven sweep AC5 rests on.
///
/// > *"Stopping any instance leaves all others functional; peers observe the
/// > departure within the liveness window."*
///
/// A peer that stops does not announce it — that is the whole difficulty — so
/// the only signal is the age of the last evidence it produced. This sweep is
/// what turns that age into an event, and it is the *only* thing in the
/// context that does: presence itself is derived on every read and stored
/// nowhere (invariant 7).
///
/// # Idempotent within one silence
///
/// A peer is reported once per stretch of quiet, however often the sweep runs;
/// fresh evidence re-arms it, so a peer that returns and goes quiet again
/// expires again. Without that, a per-second tick would emit an expiry per
/// second per departed peer.
///
/// # Sessions are untouched
///
/// Silence is not a closed link. Only the transport can report a dead session,
/// and whether an expiry should provoke a close is a decision — it belongs to
/// whoever drives this port, not to the sweep.
///
/// Nothing here crosses a context boundary: `PeerPresenceExpired` is this
/// context's own event, and no other context learns what presence is.
#[derive(Clone)]
pub struct ExpirePresenceHandler {
    state: Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    windows: LivenessWindows,
}

impl ExpirePresenceHandler {
    pub fn new(
        state: Arc<MembershipState>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
        windows: LivenessWindows,
    ) -> Self {
        Self {
            state,
            clock,
            publisher,
            windows,
        }
    }

    pub fn handle(
        &self,
        _command: ExpirePresence,
    ) -> Result<Vec<PeerPresenceExpired>, EventPublisherError> {
        let now = self.clock.now();

        let expired = self
            .state
            .modify(|roster| roster.expire_presence(now, self.windows));

        for event in &expired {
            self.publisher.publish((*event).into())?;
        }

        Ok(expired)
    }
}
