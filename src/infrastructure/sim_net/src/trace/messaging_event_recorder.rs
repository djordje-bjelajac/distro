use std::sync::Arc;

use messaging::domain::events::MessagingEvent;
use messaging::ports::{EventPublisherError, EventPublisherPort};
use shared_types::PeerId;

use crate::clock::VirtualClock;
use crate::trace::{EventTrace, TraceEvent};

/// One peer's `messaging` event publisher.
///
/// Every event this context publishes is internal to it — the read models and
/// the local diagnostic counters are the consumers — so unlike
/// [`MembershipEventRecorder`](crate::trace::MembershipEventRecorder) there is
/// nothing here to fan out to another context. Traffic between the two runs the
/// other way.
///
/// Recording is unconditional and in call order, which is what lets a scenario
/// assert on `MessageRejected` and `MessageGapClosed` — the diagnostics AC6,
/// AC14, and AC15 are about — without the application layer needing a second
/// way to expose them.
pub struct MessagingEventRecorder {
    peer: PeerId,
    clock: Arc<VirtualClock>,
    trace: Arc<EventTrace>,
}

impl MessagingEventRecorder {
    /// A publisher for `peer`, writing into `trace`.
    pub const fn new(peer: PeerId, clock: Arc<VirtualClock>, trace: Arc<EventTrace>) -> Self {
        Self { peer, clock, trace }
    }
}

impl EventPublisherPort for MessagingEventRecorder {
    fn publish(&self, event: MessagingEvent) -> Result<(), EventPublisherError> {
        self.trace.record(
            self.clock.now_millis(),
            TraceEvent::Messaging {
                peer: self.peer,
                event,
            },
        );

        Ok(())
    }
}
