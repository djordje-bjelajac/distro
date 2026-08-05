use std::fmt;

use crate::domain::{ConversationId, SequenceNumber};

/// The local peer's outbound sequence counter, one per conversation (D12,
/// AC16).
///
/// # Why this is a port and not a field
///
/// Sequence numbers are specified per `(author, conversation)`, but with
/// in-memory-only history (D7) their *state* was per-process on both sides. A
/// restarted peer therefore resumed at [`SequenceNumber::FIRST`] while every
/// peer still online held its high-water mark at N — so each message it sent
/// was, correctly by the receiver's rules, classified a duplicate and ignored.
/// The peer went permanently mute while appearing, to itself, to work.
///
/// The counter's true domain of validity is the **identity**, not the process.
///
/// # Contract: the keypair's lifetime, exactly
///
/// An implementation must keep the counter for as long as the keypair lives and
/// no longer. If the key survives a restart the counter must survive with it;
/// if the key is gone the identity is gone, and starting again at `FIRST` is
/// then correct rather than harmful. `infra-store-fs` implements this beside the
/// keystore (OP-11), with S4's schema-version discipline.
///
/// [`issue_next`](Self::issue_next) must record the advance **before** it
/// returns: a number handed out but not persisted is a number that will be
/// re-issued after a crash, which is the very failure this port exists to
/// prevent. Reporting [`NotPersisted`](SequenceCounterError::NotPersisted) and
/// sending nothing is strictly better than sending something that will be
/// ignored.
///
/// The port issues numbers for the **local** peer only. Remote authors' marks
/// are the receiving side's business and live in the conversation
/// ([`AuthorLog`](crate::domain::AuthorLog)); nothing here reads or writes
/// them.
pub trait SequenceCounterPort {
    /// Loads the counter for `conversation`, advances it, and returns the
    /// number the next locally composed message must carry.
    ///
    /// Never returns the same number twice for one conversation, across process
    /// lifetimes as well as within one.
    fn issue_next(
        &self,
        conversation: ConversationId,
    ) -> Result<SequenceNumber, SequenceCounterError>;

    /// The highest number issued so far for `conversation`; `None` when this
    /// peer has never spoken there.
    ///
    /// This is what [`Conversation::rehydrate`](crate::domain::Conversation::rehydrate)
    /// takes: it restores the mark without pretending the messages themselves
    /// survived (D7).
    fn last_issued(
        &self,
        conversation: ConversationId,
    ) -> Result<Option<SequenceNumber>, SequenceCounterError>;
}

/// Typed failure of a [`SequenceCounterPort`] operation.
///
/// Deliberately coarse and free of I/O detail: the application decides what to
/// do per variant, while adapters log the specifics they alone can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceCounterError {
    /// The counter store cannot be reached at all.
    Unavailable,
    /// The advance could not be recorded, so the number must not be used —
    /// see the port's contract.
    NotPersisted,
    /// The local peer has used every representable number in this conversation
    /// (2^64 - 1 messages). Wrapping would re-issue numbers and is never an
    /// option.
    Exhausted,
    /// The store carries a schema version this build does not understand; the
    /// original must be preserved untouched (S4).
    UnsupportedSchemaVersion { found: u32 },
}

impl fmt::Display for SequenceCounterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the sequence counter is not available"),
            Self::NotPersisted => {
                f.write_str("the advanced sequence counter could not be recorded")
            }
            Self::Exhausted => f.write_str("this conversation has no sequence number left"),
            Self::UnsupportedSchemaVersion { found } => {
                write!(
                    f,
                    "the sequence counter store has unsupported schema version {found}"
                )
            }
        }
    }
}

impl std::error::Error for SequenceCounterError {}
