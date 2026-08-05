use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use messaging::domain::{ConversationId, Message};
use messaging::ports::{MessageLogError, MessageLogPort};

/// Applied messages, in memory, for as long as the process lives (D7).
///
/// # It is in this crate precisely because it is *not* a file
///
/// Every other store here writes to disk, and the composition root gets all of
/// them from one place. History is the deliberate exception: v1 keeps no
/// durable conversation log, so a restarted peer loses what was said while its
/// identity, its trust decisions, its peer cache, and its outbound sequence
/// counter survive. Putting the in-memory log beside the durable stores makes
/// that choice visible at the point where a root might otherwise assume a file
/// exists — and a durable adapter later is a drop-in behind the same port that
/// touches no domain code.
///
/// The behaviour deliberately matches `infra-sim-net`'s log of the same name:
/// the simulator is where multi-peer claims are verified (S5), and a store that
/// behaved differently in the app than in the harness would make those
/// verifications say nothing about the app.
///
/// # Order and capacity
///
/// Messages come back in append order and conversations in [`ConversationId`]
/// order — broadcast first, then directs by peer id — which is the
/// deterministic order AC13 asks for. The bound is stated rather than enforced
/// by eviction: in-memory history has to be bounded (S6), and silently dropping
/// the oldest thing anyone said is exactly the silent loss AC11 and AC15 rule
/// out. A caller that hits the cap is told.
#[derive(Debug)]
pub struct InMemoryMessageLog {
    capacity: usize,
    conversations: Mutex<BTreeMap<ConversationId, Vec<Message>>>,
}

impl InMemoryMessageLog {
    /// Total messages one log holds before it refuses to grow.
    ///
    /// 16 384 messages of at most 16 KiB each bounds the log at a few hundred
    /// megabytes in the worst case and at a few megabytes in any realistic one,
    /// while being far more than a session of human conversation produces. It
    /// is finite so that a runaway inbound loop fails loudly instead of
    /// exhausting memory — a symmetric open network has no gatekeeper who could
    /// add the bound later (S6).
    pub const DEFAULT_CAPACITY: usize = 16_384;

    /// A log bounded at `capacity` messages.
    pub const fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            conversations: Mutex::new(BTreeMap::new()),
        }
    }

    /// How many messages the log holds across every conversation.
    pub fn len(&self) -> usize {
        Self::total(&self.guard())
    }

    /// Whether nothing has been applied yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn guard(&self) -> MutexGuard<'_, BTreeMap<ConversationId, Vec<Message>>> {
        // A test that failed an assertion while holding the lock must not turn
        // every later test into a panic with a misleading cause: the first
        // failure is the one worth reading.
        self.conversations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn total(conversations: &BTreeMap<ConversationId, Vec<Message>>) -> usize {
        conversations.values().map(Vec::len).sum()
    }
}

impl Default for InMemoryMessageLog {
    fn default() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
}

impl MessageLogPort for InMemoryMessageLog {
    fn append(&self, message: &Message) -> Result<(), MessageLogError> {
        let mut conversations = self.guard();

        if Self::total(&conversations) >= self.capacity {
            return Err(MessageLogError::CapacityExhausted);
        }

        conversations
            .entry(message.conversation())
            .or_default()
            .push(message.clone());

        Ok(())
    }

    fn load(&self, conversation: ConversationId) -> Result<Vec<Message>, MessageLogError> {
        Ok(self.guard().get(&conversation).cloned().unwrap_or_default())
    }

    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError> {
        Ok(self.guard().keys().copied().collect())
    }
}
