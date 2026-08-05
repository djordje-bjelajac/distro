use std::sync::Arc;

use crate::application::{ConversationRegistry, MessageRecorder, MessagingSettings};
use crate::domain::Message;
use crate::domain::events::{MessageGapClosed, MessageReceived, MessagingEvent};
use crate::ports::{ClockPort, MessagingCommandError};

/// Give up on every gap that has waited longer than the tolerance window
/// (rule R, AC15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloseAgedGaps;

/// Handles [`CloseAgedGaps`]: the clock-driven half of rule R.
///
/// # Why anything drives this at all
///
/// A gap means *not yet received*, so an out-of-order arrival waits. Waiting
/// without end is its own kind of loss: a message that fell off the network
/// permanently would block everything its author says afterwards. The aggregate
/// bounds the wait twice — by this tolerance window, and by the per-author
/// buffer filling — but the buffer only fills if the author keeps talking. On a
/// quiet conversation nothing would ever close the gap, and that author would
/// simply stop being heard, silently, forever. This handler is the other
/// bound, and until it is driven the first one does not exist (AC10, AC15).
///
/// # Why the released messages are re-read
///
/// The sweep reports the *abandoned ranges* and nothing else. What the close
/// released is in the conversation, from each event's `to + 1` onwards — the
/// aggregate deliberately does not clone it back out. So this handler reads it
/// from where it is, which is also what guarantees the mirror and the events
/// describe the same messages.
///
/// # Idempotent and clock-parameterised
///
/// One clock reading is taken and applied to every conversation, so a sweep is
/// one consistent moment rather than a smear across however long it takes. A
/// sweep with nothing aged does nothing and reports nothing, so a fast tick
/// costs only the pass.
#[derive(Clone)]
pub struct CloseAgedGapsHandler {
    registry: Arc<ConversationRegistry>,
    settings: MessagingSettings,
    clock: Arc<dyn ClockPort + Send + Sync>,
    recorder: MessageRecorder,
}

impl CloseAgedGapsHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        settings: MessagingSettings,
        clock: Arc<dyn ClockPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            registry,
            settings,
            clock,
            recorder,
        }
    }

    pub fn handle(
        &self,
        _command: CloseAgedGaps,
    ) -> Result<Vec<MessageGapClosed>, MessagingCommandError> {
        let now = self.clock.now();
        let tolerance = self.settings.gap_tolerance;

        let swept = self.registry.sweep(|open| {
            let closed = open.close_aged_gaps(now, tolerance);

            // Everything above an abandoned range is what the close made
            // visible: before it, this author's applied run stopped below the
            // range's start.
            let released: Vec<Message> = closed
                .iter()
                .flat_map(|event| {
                    open.messages_by(&event.author)
                        .iter()
                        .filter(|message| message.sequence() > event.to)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect();

            (closed, released)
        });

        let mut all_closed = Vec::new();

        for (closed, released) in swept {
            if closed.is_empty() {
                continue;
            }

            // The abandonment comes first: it is what explains the jump in the
            // messages that follow, and a consumer given the other order would
            // have to reason backwards from a hole it had already drawn.
            let mut events: Vec<MessagingEvent> =
                closed.iter().copied().map(MessagingEvent::from).collect();
            events.extend(released.iter().map(|message| {
                MessagingEvent::from(MessageReceived {
                    id: message.id(),
                    claimed_sent_at: message.claimed_sent_at(),
                })
            }));

            self.recorder.record(&released, &events)?;
            all_closed.extend(closed);
        }

        Ok(all_closed)
    }
}
