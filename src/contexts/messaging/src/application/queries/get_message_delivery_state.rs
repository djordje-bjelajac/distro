use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::domain::{DeliveryState, Message, MessageId};

/// Ask what is known about one message's delivery (AC11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetMessageDeliveryState {
    pub id: MessageId,
}

/// Handles [`GetMessageDeliveryState`]: `pending`, `delivered`,
/// `failed(reason)`, or `published`.
///
/// # Why `None` is not "unknown delivery"
///
/// `None` means no applied message carries that identifier — it was never
/// applied, is still held behind a gap, or belongs to a conversation this
/// process has not touched. It is never "the message exists but its state is a
/// mystery": AC11 makes silent loss a non-state, so every message that exists
/// has a state, and every 1:1 message that stopped being pending has told the
/// user which of the two endings it reached.
///
/// Read from the live conversation rather than the log, because that is where
/// the state moves after the message was written — see
/// [`GetConversationHistory`](crate::application::queries::GetConversationHistory).
/// Asking about a conversation nobody has spoken in opens nothing.
#[derive(Clone)]
pub struct GetMessageDeliveryStateHandler {
    registry: Arc<ConversationRegistry>,
}

impl GetMessageDeliveryStateHandler {
    pub(crate) const fn new(registry: Arc<ConversationRegistry>) -> Self {
        Self { registry }
    }

    pub fn handle(&self, query: GetMessageDeliveryState) -> Option<DeliveryState> {
        self.registry
            .read(query.id.conversation(), |open| {
                open.message(&query.id).map(Message::delivery_state)
            })
            .flatten()
    }
}
