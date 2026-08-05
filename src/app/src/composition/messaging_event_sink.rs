use std::sync::Arc;

use messaging::domain::events::MessagingEvent;
use messaging::ports::{EventPublisherError, EventPublisherPort};

use crate::composition::{Diagnostics, GapLedger, abandoned_span};

/// `messaging`'s `EventPublisherPort`: where this context's six events become
/// the numbers and the markers a user can see.
///
/// # Why this applies events instead of queueing them
///
/// `MembershipEventRelay` queues, because its events have to *cross a context
/// boundary* and the root decides which thread that happens on. These do not
/// cross anything — canvas §2.3: "All six are context-internal" — so there is
/// nothing to schedule. Applying them where they are published also means a gap
/// marker exists the instant the gap closes, including when the close happened
/// inside an `accept_envelope` on the engine thread because a buffer filled
/// (`GapCloseCause::BufferFull`). A queued design would have shown the
/// conversation's jump before its explanation.
///
/// # What is deliberately not recorded
///
/// `MessageSent`, `MessageReceived` and `MessageDeliveryStateChanged` change
/// only things the read model already reports: `MessagingQueryPort::history`
/// has the message and `delivery_state` has its state, both read fresh on every
/// redraw. Mirroring them here would be a second copy of the conversation that
/// could disagree with the first.
///
/// What *is* recorded is exactly what the read model cannot answer:
///
/// * `MessageGapClosed` → the ledger, because the applied run shows a jump and
///   nothing else (AC15);
/// * `MessageRejected` and `MessageDuplicateIgnored` → counters, because a
///   refused envelope reaches no read model at all by construction
///   (invariant 10, AC6/AC7/AC14).
pub struct MessagingEventSink {
    gaps: Arc<GapLedger>,
    diagnostics: Arc<Diagnostics>,
}

impl MessagingEventSink {
    pub const fn new(gaps: Arc<GapLedger>, diagnostics: Arc<Diagnostics>) -> Self {
        Self { gaps, diagnostics }
    }
}

impl EventPublisherPort for MessagingEventSink {
    fn publish(&self, event: MessagingEvent) -> Result<(), EventPublisherError> {
        match event {
            MessagingEvent::MessageGapClosed(closed) => {
                self.diagnostics
                    .count_gap_abandoned(abandoned_span(&closed));
                self.gaps.record(closed);
            }
            MessagingEvent::MessageRejected(_) => self.diagnostics.count_envelope_refused(),
            MessagingEvent::MessageDuplicateIgnored(_) => {
                self.diagnostics.count_duplicate_ignored();
            }
            MessagingEvent::MessageSent(_)
            | MessagingEvent::MessageReceived(_)
            | MessagingEvent::MessageDeliveryStateChanged(_) => {}
        }

        // Nothing here can fail: a counter and a bounded in-memory ledger have
        // nothing to refuse. The port keeps its `Result` because an
        // implementation that wrote somewhere fallible would need one, and a
        // publisher that could not accept events would leave a change made but
        // unannounced.
        Ok(())
    }
}

impl std::fmt::Debug for MessagingEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessagingEventSink").finish_non_exhaustive()
    }
}
