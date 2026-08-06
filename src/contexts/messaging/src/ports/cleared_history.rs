/// What clearing the conversation history threw away (canvas `0013`).
///
/// Two counts rather than one, because they answer different questions and a
/// user asking "did that do anything?" is served by neither alone. A peer that
/// has spoken to six others but said nothing since launch drops six
/// conversations and no messages; a peer in one long thread drops one
/// conversation and hundreds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClearedHistory {
    /// How many conversations were open in this process and are not any more.
    ///
    /// The count includes conversations with no messages in them: a
    /// conversation exists here once anything has touched it, and a user who
    /// cleared one is owed the truth that it is gone.
    pub conversations_dropped: usize,
    /// How many applied messages the log was holding.
    ///
    /// The log's count, not the registry's. They are the same number in a
    /// healthy process — the log mirrors what was applied — and a divergence
    /// would be worth knowing about rather than papering over.
    pub messages_dropped: usize,
}

impl ClearedHistory {
    /// Whether there was anything to clear.
    ///
    /// The interface uses this to keep from claiming an outcome it did not
    /// produce: pressing the key on a fresh instance should say so, not report
    /// a successful clear of nothing.
    pub const fn is_empty(&self) -> bool {
        self.conversations_dropped == 0 && self.messages_dropped == 0
    }
}
