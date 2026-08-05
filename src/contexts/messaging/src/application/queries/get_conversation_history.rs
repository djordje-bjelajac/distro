use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::domain::{AuthorLog, ConversationId, Message};

/// Ask for everything visible in one conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetConversationHistory {
    pub conversation: ConversationId,
}

/// Handles [`GetConversationHistory`]: one conversation's applied messages.
///
/// # Buffered messages are not here, and cannot be
///
/// The handler reads `AuthorLog::messages`, which is the *applied* run and
/// nothing else. A message waiting for a gap to close is not part of the
/// conversation yet (invariant 5), and showing it would display an author's
/// messages out of that author's own send order — the single thing the
/// sequencing rules exist to prevent (AC8). This is a property of what is read,
/// not a filter that could be forgotten: the aggregate does not expose held
/// messages at all.
///
/// Content that failed signature verification is equally unreachable: it is
/// refused at the boundary and never reaches a conversation (invariant 10, AC6).
///
/// # Why the live conversation rather than the log
///
/// Delivery state moves *after* a message is written — a direct message
/// appended as `pending` becomes `delivered` or `failed` later — and
/// `MessageLogPort` has no update method, because the canvas declares none. The
/// log is a mirror for a future durable adapter (D7); the conversation is the
/// live truth, and AC11 asks for the live truth.
///
/// # The order, and the order that does not exist
///
/// Grouped by author in `PeerId` order, and within an author in that author's
/// send order. There is no ordering *across* authors and none is invented:
/// with no global clock, no consensus, and an author's claimed send time being
/// theirs to fabricate, there is nothing to derive one from. AC8 asks only that
/// one author's messages hold their own order, which is exactly what sequence
/// numbers decide.
///
/// # It reads, and only reads
///
/// A conversation this peer has never seen returns empty rather than being
/// brought into existence — rendering must not change what
/// [`ListConversations`](crate::application::queries::ListConversations)
/// reports. Absence of history is not an error, and a late joiner seeing
/// nothing said before it arrived is correct (AC10).
#[derive(Clone)]
pub struct GetConversationHistoryHandler {
    registry: Arc<ConversationRegistry>,
}

impl GetConversationHistoryHandler {
    pub(crate) const fn new(registry: Arc<ConversationRegistry>) -> Self {
        Self { registry }
    }

    pub fn handle(&self, query: GetConversationHistory) -> Vec<Message> {
        self.registry
            .read(query.conversation, |open| {
                open.logs().flat_map(AuthorLog::messages).cloned().collect()
            })
            .unwrap_or_default()
    }
}
