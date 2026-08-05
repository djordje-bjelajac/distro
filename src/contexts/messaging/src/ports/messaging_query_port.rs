use crate::domain::{ConversationId, DeliveryState, Message, MessageId};
use crate::ports::MessageLogError;

/// The **inbound** (driving) read contract of `messaging` (canvas §4, inbound
/// column).
///
/// Every method reads and returns; none writes, and this crate's query tests
/// assert that rather than trust it — including that asking about a
/// conversation nobody has spoken in does not bring one into existence.
///
/// # What a read may never show
///
/// Buffered arrivals. A message held waiting for a gap to close is not part of
/// any conversation yet (invariant 5), and showing it would display an author's
/// messages out of that author's send order — the one thing the sequencing
/// rules exist to prevent (AC8). The read model is the *applied* run and
/// nothing else.
///
/// Content that failed signature verification is likewise unreachable here: it
/// is refused at the boundary and never reaches a conversation (invariant 10,
/// AC6).
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn MessagingQueryPort + Send + Sync>`.
pub trait MessagingQueryPort {
    /// Every conversation with recorded history, in a deterministic order
    /// (AC13) — `Broadcast` first, then direct conversations by `PeerId`.
    ///
    /// The only method here that can fail, because it is the only one that
    /// reads the message log rather than the live conversations.
    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError>;

    /// One conversation's applied messages.
    ///
    /// Grouped by author in `PeerId` order, and within an author in that
    /// author's send order. There is no order *across* authors, and none is
    /// invented: with no global clock and no consensus there is nothing to
    /// derive one from, and AC8 asks only that one author's messages hold their
    /// own order.
    ///
    /// Empty for a conversation this peer has never seen — absence of history
    /// is not an error, and a late joiner seeing nothing said before it arrived
    /// is correct (AC10).
    fn history(&self, conversation: ConversationId) -> Vec<Message>;

    /// What is known about one message's delivery (AC11); `None` if no applied
    /// message carries that identifier.
    fn delivery_state(&self, id: MessageId) -> Option<DeliveryState>;
}
