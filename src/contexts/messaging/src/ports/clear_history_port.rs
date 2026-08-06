use crate::ports::{ClearedHistory, MessageLogError};

/// The **inbound** (driving) contract for throwing this process's conversation
/// history away (canvas `0013`, D6).
///
/// # Why this is its own port
///
/// The three inbound ports that already exist each answer a different question,
/// and a clear is none of them. [`SendMessagePort`](crate::ports::SendMessagePort)
/// is the *composing* port, whose whole design is keeping the direct and
/// broadcast paths separate end to end — a clear is neither path.
/// [`InboundEnvelopePort`](crate::ports::InboundEnvelopePort) carries what the
/// network reports and what the root's tick drives; nobody sent this.
/// [`MessagingQueryPort`](crate::ports::MessagingQueryPort) only reads.
///
/// `membership` needed no equivalent, because its `JoinNetworkPort` is already
/// "the decisions a person makes" and forgetting peers is a fourth one of
/// exactly that kind. This context has no such port, so rather than widening
/// one whose own doc argues against it, there is this.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn ClearHistoryPort + Send + Sync>`.
pub trait ClearHistoryPort {
    /// Drops every conversation this process holds and empties the log.
    ///
    /// # What survives, and why it must
    ///
    /// The outbound sequence counter. It is not history — it records what this
    /// identity has issued, and every peer still online is holding that mark.
    /// Resetting it would make every later message a duplicate to them, and
    /// this peer would go mute while its own screen looked fine (D12, AC16).
    /// Conversations reopened after a clear rehydrate from the counter and
    /// resume above their old mark.
    ///
    /// Trust, blocks and verification also survive: they belong to `identity`
    /// and clearing a screen is not a reason to unblock anybody.
    ///
    /// # It is quiet
    ///
    /// No domain event is published. Nothing outside this process may learn
    /// that a user cleared their screen — and no gap is reported for the
    /// messages that went, because a cleared log has no record of ever having
    /// been in the middle of a stream.
    ///
    /// # What a caller is buying
    ///
    /// Exactly-once application (AC7) is scoped to a run rather than to an
    /// identity from here on: a message this peer has already applied will be
    /// applied again if it arrives again after a clear. Every signature is
    /// still verified and every author policy still applies, so what is
    /// re-armed is redundant display, never forged content.
    fn clear_history(&self) -> Result<ClearedHistory, MessageLogError>;
}
