use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::ports::{ClearedHistory, MessageLogError, MessageLogPort};

/// Throw away every conversation this process is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClearHistory;

/// Handles [`ClearHistory`]: drop the conversations, then empty the log.
///
/// # Two stores, one operation
///
/// The registry holds the aggregates the interface renders; the log mirrors
/// what has been applied and is what the interface *lists*. Clear one and not
/// the other and the read model contradicts itself — conversation rows whose
/// history loads as empty, or an empty list beside a pane still showing
/// messages. So both move here, registry first: it cannot fail, and doing the
/// fallible half second means a log that refuses leaves the user with less
/// history than they had rather than more.
///
/// # What this must never touch
///
/// [`SequenceCounterPort`](crate::ports::SequenceCounterPort). Not "should
/// not" — there is deliberately no counter in this handler's fields, so
/// resetting one would take an edit large enough to notice. The mark records
/// what this identity has *issued*, every peer still online is holding it, and
/// resetting it would have every later message classified a duplicate by all
/// of them: this peer going mute while its own screen looks perfectly healthy
/// (D12, AC16). A conversation reopened after a clear rehydrates from the
/// counter and resumes above its old mark, which is the whole reason dropping
/// the registry is safe.
///
/// Trust, blocks, verification and the keypair are other contexts' and are
/// equally absent.
///
/// # It publishes nothing
///
/// No event, for two reasons. Nothing outside this process may learn that a
/// user cleared their screen — that is a privacy property, and it is free only
/// because there is no event here to leak. And no gap is reported for what
/// went: a cleared log holds no record of having been mid-stream, so the next
/// sequence it sees establishes a fresh origin rather than reporting the
/// forgotten run as loss (D10).
#[derive(Clone)]
pub struct ClearHistoryHandler {
    registry: Arc<ConversationRegistry>,
    log: Arc<dyn MessageLogPort + Send + Sync>,
}

impl ClearHistoryHandler {
    pub(crate) fn new(
        registry: Arc<ConversationRegistry>,
        log: Arc<dyn MessageLogPort + Send + Sync>,
    ) -> Self {
        Self { registry, log }
    }

    pub fn handle(&self, _command: ClearHistory) -> Result<ClearedHistory, MessageLogError> {
        let conversations_dropped = self.registry.clear();
        let messages_dropped = self.log.clear()?;

        Ok(ClearedHistory {
            conversations_dropped,
            messages_dropped,
        })
    }
}
