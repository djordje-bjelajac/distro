use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use membership::domain::events::MembershipEvent;
use membership::ports::{EventPublisherError, EventPublisherPort};
use shared_types::PeerId;

use crate::clock::VirtualClock;
use crate::stores::guard;
use crate::trace::{EventTrace, TraceEvent};

/// One peer's `membership` event publisher: it records everything and queues
/// the two events that leave the context.
///
/// # Why the cross-context events are queued rather than forwarded
///
/// `PeerConnected` and `PeerDisconnected` have to reach `messaging`'s
/// `PeerLifecyclePort` — that fan-out is the entire coupling between the two
/// contexts (canvas §4, D10). Doing it inside `publish` is not possible and
/// would be wrong twice over:
///
/// * **Construction order.** `MembershipContext::new` needs this publisher, and
///   `MessagingContext::new` needs the transport; a publisher holding the
///   messaging context could never be built first.
/// * **Re-entrancy.** `publish` is called from inside a `membership` command
///   handler. Calling into `messaging` from there would run a second context's
///   command inside the first's, in an order no scenario could predict, and
///   with a lock discipline neither context designed for.
///
/// So the events queue here and the harness's pump drains them between frame
/// deliveries — which is what a composition root does with a subscription, and
/// what makes the fan-out a visible, ordered step rather than a hidden call.
///
/// Context-internal events (`NetworkJoined`, `PeerDiscovered`, …) are traced
/// and go no further: `is_cross_context` is the domain's own statement of
/// which ones leave, and leaking the rest would hand `messaging` endpoints and
/// sessions it must never see.
pub struct MembershipEventRecorder {
    peer: PeerId,
    clock: Arc<VirtualClock>,
    trace: Arc<EventTrace>,
    pending: Mutex<VecDeque<MembershipEvent>>,
}

impl MembershipEventRecorder {
    /// A publisher for `peer`, writing into `trace`.
    pub fn new(peer: PeerId, clock: Arc<VirtualClock>, trace: Arc<EventTrace>) -> Self {
        Self {
            peer,
            clock,
            trace,
            pending: Mutex::new(VecDeque::new()),
        }
    }

    /// Takes the cross-context events published since the last drain, in
    /// publication order.
    ///
    /// Order matters: a `PeerDisconnected` that overtook its `PeerConnected`
    /// would leave `messaging` believing a dead peer is live.
    pub fn drain_cross_context(&self) -> Vec<MembershipEvent> {
        guard(&self.pending).drain(..).collect()
    }

    /// Whether anything is waiting to be fanned out.
    pub fn has_pending(&self) -> bool {
        !guard(&self.pending).is_empty()
    }
}

impl EventPublisherPort for MembershipEventRecorder {
    fn publish(&self, event: MembershipEvent) -> Result<(), EventPublisherError> {
        self.trace.record(
            self.clock.now_millis(),
            TraceEvent::Membership {
                peer: self.peer,
                event,
            },
        );

        if event.is_cross_context() {
            guard(&self.pending).push_back(event);
        }

        Ok(())
    }
}
