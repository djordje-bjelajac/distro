use shared_types::PeerId;

use crate::domain::{ConversationId, SequenceNumber};

/// What makes a message *that* message: its author, its conversation, and the
/// author's sequence number within it (canvas §2.3).
///
/// All three parts are load-bearing. Sequence numbers are counted per
/// `(author, conversation)`, so an identifier missing either part would
/// collapse unrelated messages together and make the dedup rule (invariant 6)
/// discard real content.
///
/// The `author` is the peer whose signature verified on the envelope, never a
/// field a sender chose (invariant 4). Nothing in this type can enforce that —
/// it is a precondition of every path that builds one from the wire, stated on
/// [`Conversation::accept_remote`](crate::domain::Conversation::accept_remote).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId {
    author: PeerId,
    conversation: ConversationId,
    sequence: SequenceNumber,
}

impl MessageId {
    pub const fn new(
        author: PeerId,
        conversation: ConversationId,
        sequence: SequenceNumber,
    ) -> Self {
        Self {
            author,
            conversation,
            sequence,
        }
    }

    pub const fn author(&self) -> PeerId {
        self.author
    }

    pub const fn conversation(&self) -> ConversationId {
        self.conversation
    }

    pub const fn sequence(&self) -> SequenceNumber {
        self.sequence
    }
}
