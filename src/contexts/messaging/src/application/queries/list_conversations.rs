use std::sync::Arc;

use crate::domain::ConversationId;
use crate::ports::{MessageLogError, MessageLogPort};

/// Ask which conversations have anything in them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListConversations;

/// Handles [`ListConversations`]: the conversations the message log holds
/// history for, in a stable order.
///
/// # Why the log and not the live conversations
///
/// The registry opens a conversation the moment anything touches it — a session
/// establishing, a send being prepared — so listing from there would show
/// conversations nobody has said anything in. The log holds applied messages
/// only, which is the honest definition of a conversation that exists.
///
/// It is also the seam D7 leaves open: history is in memory today and a durable
/// adapter is a later drop-in behind this port. Reading the list from the port
/// means that adapter changes what the user sees on the next launch without
/// this handler changing at all.
///
/// # Sorted deliberately
///
/// A log's order is implementation-defined; sorting makes the listing
/// deterministic (AC13, S5) rather than dependent on insertion history.
/// `Broadcast` sorts first, then direct conversations by `PeerId` — never by
/// display name, which takes no part in identity or lookup (invariant 8).
#[derive(Clone)]
pub struct ListConversationsHandler {
    log: Arc<dyn MessageLogPort + Send + Sync>,
}

impl ListConversationsHandler {
    pub(crate) const fn new(log: Arc<dyn MessageLogPort + Send + Sync>) -> Self {
        Self { log }
    }

    pub fn handle(
        &self,
        _query: ListConversations,
    ) -> Result<Vec<ConversationId>, MessageLogError> {
        let mut conversations = self.log.conversations()?;
        conversations.sort_unstable();
        conversations.dedup();

        Ok(conversations)
    }
}
