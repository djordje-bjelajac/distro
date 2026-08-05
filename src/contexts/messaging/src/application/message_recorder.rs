use std::sync::Arc;

use crate::domain::Message;
use crate::domain::events::MessagingEvent;
use crate::ports::{EventPublisherPort, MessageLogPort, MessagingCommandError};

/// Carries out the two things that must happen after a conversation changes:
/// mirror the messages it applied into the log, then announce what happened.
///
/// The aggregate holds no ports — it returns events and the application
/// delivers them — and three handlers need to deliver them the same way.
/// Sharing one implementation is what keeps the log and the event stream from
/// being written in three slightly different orders.
///
/// # Log first, then events
///
/// A consumer reacting to `MessageReceived` may go and read; it must not find
/// less than the event promised. The reverse order would make that a race for
/// anything reading through the log.
///
/// # Only applied messages
///
/// Buffered arrivals are never handed here. They are not part of a conversation
/// yet (invariant 5), and a log that held them would resurrect them out of
/// order after a restart — which is exactly what `MessageLogPort` forbids.
#[derive(Clone)]
pub(crate) struct MessageRecorder {
    log: Arc<dyn MessageLogPort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
}

impl MessageRecorder {
    pub(crate) const fn new(
        log: Arc<dyn MessageLogPort + Send + Sync>,
        publisher: Arc<dyn EventPublisherPort + Send + Sync>,
    ) -> Self {
        Self { log, publisher }
    }

    /// Mirrors `applied` into the log, then publishes `events` in order.
    ///
    /// Order is significant across events: a `MessageReceived` that overtook
    /// the one before it would show a conversation out of order, which is the
    /// one thing the sequencing rules exist to prevent (AC8).
    pub(crate) fn record(
        &self,
        applied: &[Message],
        events: &[MessagingEvent],
    ) -> Result<(), MessagingCommandError> {
        for message in applied {
            self.log.append(message)?;
        }
        for event in events {
            self.publisher.publish(*event)?;
        }

        Ok(())
    }

    /// Publishes one event with nothing to mirror — a delivery state change,
    /// or a refusal that never produced a message.
    pub(crate) fn announce(
        &self,
        event: impl Into<MessagingEvent>,
    ) -> Result<(), MessagingCommandError> {
        self.publisher.publish(event.into())?;
        Ok(())
    }
}
