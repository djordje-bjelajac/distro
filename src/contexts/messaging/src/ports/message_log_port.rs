use std::fmt;

use crate::domain::{ConversationId, Message};

/// Where applied messages are kept (canvas §4, D7).
///
/// v1 stores history in memory only, so a conversation dies with the process; a
/// durable adapter is a later drop-in behind this trait that touches no domain
/// code. The keypair and peer cache *do* persist, but those belong to
/// `identity` and `membership` — message history is this context's, and this
/// context has decided it is not worth a file.
///
/// Only *applied* messages come here. Buffered ones are not part of any
/// conversation yet (invariant 5) and must never be written, or a restart would
/// resurrect them out of order.
pub trait MessageLogPort {
    /// Records one applied message.
    ///
    /// The conversation is not a separate argument: a
    /// [`Message`] already carries it inside its identifier, and passing both
    /// would let a caller state two different things at once.
    fn append(&self, message: &Message) -> Result<(), MessageLogError>;

    /// Every message stored for one conversation, in append order.
    ///
    /// A conversation nobody has spoken in yet loads as empty rather than
    /// failing: absence of history is not an error.
    fn load(&self, conversation: ConversationId) -> Result<Vec<Message>, MessageLogError>;

    /// Every conversation the log holds anything for, in a deterministic
    /// order (AC13) — the input to the `ListConversations` query (OP-7).
    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError>;

    /// Discards everything and reports how many messages went.
    ///
    /// The user-driven half of a clear (canvas `0013`). It is not a prune and
    /// not an eviction: there is no age, no cap, and no selection — either the
    /// log holds a conversation or it holds nothing.
    ///
    /// # It never travels alone
    ///
    /// This log is a *mirror* of what the application has applied, and
    /// [`conversations`](Self::conversations) is what the interface lists. A
    /// log cleared while the conversations it mirrors are still open leaves the
    /// listing and the history disagreeing — rows on screen whose contents
    /// load as empty. Whoever calls this must clear the conversations in the
    /// same operation.
    ///
    /// # What it must not reach
    ///
    /// The outbound sequence counter. That mark is not history: it records
    /// what this identity has *issued*, and peers still online are holding it.
    /// A clear that reset it would have every later message classified a
    /// duplicate by those peers — this peer going mute while its own screen
    /// looks perfectly healthy (D12). A conversation reopened after a clear
    /// rehydrates from the counter and picks up exactly where it left off,
    /// which is the behaviour, not an accident of one.
    ///
    /// Clearing an empty log is `Ok(0)`, not an error: having nothing to
    /// forget is not a failure to forget.
    fn clear(&self) -> Result<usize, MessageLogError>;
}

/// Typed failure of a [`MessageLogPort`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLogError {
    /// The log cannot be reached at all.
    Unavailable,
    /// The log is at its cap and will not grow. In-memory history has to be
    /// bounded (D7, S6), and reaching the bound is a stated condition rather
    /// than a quiet eviction of the oldest thing anyone said.
    CapacityExhausted,
}

impl fmt::Display for MessageLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the message log is not available"),
            Self::CapacityExhausted => f.write_str("the message log has no room for more messages"),
        }
    }
}

impl std::error::Error for MessageLogError {}
