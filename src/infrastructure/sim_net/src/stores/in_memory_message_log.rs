use std::collections::BTreeMap;
use std::sync::Mutex;

use messaging::domain::{ConversationId, Message};
use messaging::ports::{MessageLogError, MessageLogPort};

use crate::stores::guard;

/// The applied-message mirror, in memory (D7).
///
/// # It dies with the process, deliberately
///
/// v1 keeps no durable history, so the harness builds a *fresh* log on every
/// peer rebuild. That is not a shortcut — it is the exact condition D12 was
/// written for: a restarted peer loses its conversations while its identity and
/// its outbound sequence counter survive, and AC16 asks that it still be heard.
/// A log that quietly survived a restart here would make that scenario pass for
/// the wrong reason.
///
/// # Order and capacity
///
/// Messages are returned in append order and conversations in `ConversationId`
/// order — `Broadcast` first, then directs by `PeerId` — which is the
/// deterministic order AC13 requires. The capacity bound is stated rather than
/// enforced by eviction: in-memory history has to be bounded (S6), and
/// silently dropping the oldest thing anyone said is exactly the silent loss
/// AC11 and AC15 rule out.
#[derive(Debug)]
pub struct InMemoryMessageLog {
    capacity: usize,
    conversations: Mutex<BTreeMap<ConversationId, Vec<Message>>>,
}

impl InMemoryMessageLog {
    /// Total messages one log holds before it refuses to grow.
    ///
    /// Generous for a scenario — no deterministic test writes thousands of
    /// messages — and finite, so a runaway loop in a future scenario fails
    /// loudly instead of exhausting memory.
    pub const DEFAULT_CAPACITY: usize = 4_096;

    /// A log bounded at `capacity` messages.
    pub const fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            conversations: Mutex::new(BTreeMap::new()),
        }
    }

    /// How many messages the log holds across every conversation.
    pub fn len(&self) -> usize {
        guard(&self.conversations)
            .values()
            .map(Vec::len)
            .sum::<usize>()
    }

    /// Whether nothing has been applied yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemoryMessageLog {
    fn default() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
}

impl MessageLogPort for InMemoryMessageLog {
    fn append(&self, message: &Message) -> Result<(), MessageLogError> {
        let mut conversations = guard(&self.conversations);

        if conversations.values().map(Vec::len).sum::<usize>() >= self.capacity {
            return Err(MessageLogError::CapacityExhausted);
        }

        conversations
            .entry(message.conversation())
            .or_default()
            .push(message.clone());

        Ok(())
    }

    fn load(&self, conversation: ConversationId) -> Result<Vec<Message>, MessageLogError> {
        Ok(guard(&self.conversations)
            .get(&conversation)
            .cloned()
            .unwrap_or_default())
    }

    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError> {
        Ok(guard(&self.conversations).keys().copied().collect())
    }
}
