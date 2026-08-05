use std::sync::Arc;

use membership::domain::events::MembershipEvent;
use messaging::ports::PeerLifecyclePort;

use crate::composition::{Diagnostics, NoticeFeed};

/// The one seam between two contexts, and the only thing that crosses it.
///
/// # What the canvas asks for here
///
/// > *`messaging` ↔ `membership` interaction happens only via
/// > `PeerConnected`/`PeerDisconnected` (`shared_types`, carrying `PeerId`
/// > only) and `messaging`'s own `MessageTransportPort`.* — canvas §4
///
/// `membership` publishes its events to a port; `messaging` consumes exactly
/// two of them through a port of its own; neither imports the other. This type
/// is the piece in between, and it is deliberately the *only* place a
/// `MembershipEvent` and a `PeerLifecyclePort` are named together.
///
/// # Why the filter is the event's own
///
/// [`MembershipEvent::is_cross_context`] exists for this: it answers whether an
/// event leaves the context, and adding a variant to that enum makes every
/// exhaustive consumer fail to compile. Re-deciding it here — by matching the
/// two variants directly — would be a second answer that could drift from the
/// first the day a third cross-context event is added.
///
/// # Why a missing fan-out is not merely a missing notification
///
/// Without a caller on `PeerLifecyclePort`, a direct message handed to a
/// transport whose session then dies stays `Pending` forever, which AC11 calls
/// silent loss wearing a spinner (D10). And without `peer_connected` a
/// conversation is not rehydrated from the sequence counter, so a restarted
/// peer re-uses numbers its listeners already hold and goes permanently mute
/// (D12, AC16). Both are stated on the port; this is what makes them true.
pub struct LifecycleFanout {
    lifecycle: Arc<dyn PeerLifecyclePort + Send + Sync>,
    diagnostics: Arc<Diagnostics>,
    notices: Arc<NoticeFeed>,
}

impl LifecycleFanout {
    pub const fn new(
        lifecycle: Arc<dyn PeerLifecyclePort + Send + Sync>,
        diagnostics: Arc<Diagnostics>,
        notices: Arc<NoticeFeed>,
    ) -> Self {
        Self {
            lifecycle,
            diagnostics,
            notices,
        }
    }

    /// Fans one membership event into `messaging`, if it is one that crosses.
    ///
    /// Context-internal events — a discovery, a join, a presence expiry — are
    /// passed over: `messaging` must never learn what an `Endpoint`, a session,
    /// or a presence is.
    pub fn fan(&self, event: &MembershipEvent) {
        if !event.is_cross_context() {
            return;
        }

        let refused = match event {
            MembershipEvent::PeerConnected(connected) => self
                .lifecycle
                .peer_connected(*connected)
                .err()
                .map(|error| format!("a peer became reachable but messaging refused: {error}")),
            MembershipEvent::PeerDisconnected(disconnected) => self
                .lifecycle
                .peer_disconnected(*disconnected)
                .err()
                .map(|error| {
                    format!(
                        "a peer went away but its pending messages could not be failed: {error}"
                    )
                }),
            // Unreachable while `is_cross_context` and this match agree, and
            // matched exhaustively so that a third cross-context event has to
            // be considered here rather than silently dropped.
            MembershipEvent::NetworkJoined(_)
            | MembershipEvent::NetworkLeft(_)
            | MembershipEvent::PeerDiscovered(_)
            | MembershipEvent::PeerPresenceExpired(_) => None,
        };

        if let Some(message) = refused {
            self.diagnostics.count_port_refusal();
            self.notices.warn(message);
        }
    }
}

impl std::fmt::Debug for LifecycleFanout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleFanout").finish_non_exhaustive()
    }
}
