use std::fmt;

use crate::domain::{ConversationError, SequenceNumber};
use crate::ports::{
    EnvelopeSignerError, EnvelopeVerifierError, EventPublisherError, MessageLogError,
    MessageTransportError, SequenceCounterError,
};

/// Why a `messaging` command could not be carried out.
///
/// One error type across the context's three command ports, because a caller's
/// response depends on *which collaborator* refused rather than on which
/// command it was running.
///
/// # What is deliberately not in here
///
/// A **direct** send that the transport refuses is not an error. AC11 makes
/// silent loss a non-state, so the message is recorded, marked
/// [`Failed`](crate::domain::DeliveryState::Failed) with the transport's
/// reason, and returned as a successful outcome the user can see and act on —
/// returning `Err` there would throw away the very record AC11 asks for.
/// [`Transport`](Self::Transport) therefore reaches a caller only from the
/// **broadcast** path, which has no failed delivery state at all (D3, AC10):
/// gossip that never accepted a message must not leave this peer claiming it
/// was published.
///
/// A signature that does not verify is not in here either — it is a
/// [`MessageRejected`](crate::domain::events::MessageRejected), which is data.
/// [`Verifier`](Self::Verifier) means the check could not be *performed*, which
/// is a different fact (AC6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagingCommandError {
    /// The conversation refused the operation. The caller's view is wrong;
    /// retrying changes nothing.
    Conversation(ConversationError),
    /// The outbound sequence counter could not issue or report a number. No
    /// message was composed — see `SequenceCounterPort`'s contract on why a
    /// number that was not recorded must not be used (D12).
    Sequence(SequenceCounterError),
    /// The counter and the conversation disagree about the local peer's next
    /// sequence number.
    ///
    /// Unreachable while the two are kept in step — every conversation is
    /// rehydrated from the counter on first touch (D12/AC16) — and checked
    /// anyway, before anything is recorded or sent. The alternative to failing
    /// here is emitting a message whose wire sequence differs from the one its
    /// local identifier carries, which corrupts ordering for every receiver
    /// and is invisible to the sender.
    SequenceDiverged {
        /// What the counter issued.
        issued: SequenceNumber,
        /// What the conversation would have assigned.
        recorded: SequenceNumber,
    },
    /// The envelope could not be signed, so nothing was recorded or sent: an
    /// unsigned envelope has no author (invariant 4) and no peer would accept
    /// it.
    Signer(EnvelopeSignerError),
    /// The signature check could not be performed, leaving authenticity
    /// unknown — never "invalid", and never "valid" (AC6).
    Verifier(EnvelopeVerifierError),
    /// The broadcast channel would not take the message. Direct sends never
    /// produce this — see the type docs.
    Transport(MessageTransportError),
    /// The message log refused the mirror. The conversation already changed,
    /// so this is the one variant that leaves durable history behind the live
    /// view (D7).
    Log(MessageLogError),
    /// The change was made but could not be announced.
    Publisher(EventPublisherError),
}

impl From<ConversationError> for MessagingCommandError {
    fn from(error: ConversationError) -> Self {
        Self::Conversation(error)
    }
}

impl From<SequenceCounterError> for MessagingCommandError {
    fn from(error: SequenceCounterError) -> Self {
        Self::Sequence(error)
    }
}

impl From<EnvelopeSignerError> for MessagingCommandError {
    fn from(error: EnvelopeSignerError) -> Self {
        Self::Signer(error)
    }
}

impl From<EnvelopeVerifierError> for MessagingCommandError {
    fn from(error: EnvelopeVerifierError) -> Self {
        Self::Verifier(error)
    }
}

impl From<MessageTransportError> for MessagingCommandError {
    fn from(error: MessageTransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<MessageLogError> for MessagingCommandError {
    fn from(error: MessageLogError) -> Self {
        Self::Log(error)
    }
}

impl From<EventPublisherError> for MessagingCommandError {
    fn from(error: EventPublisherError) -> Self {
        Self::Publisher(error)
    }
}

impl fmt::Display for MessagingCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conversation(error) => write!(f, "{error}"),
            Self::Sequence(error) => write!(f, "{error}"),
            Self::SequenceDiverged { issued, recorded } => write!(
                f,
                "the counter issued sequence {issued} but the conversation would record {recorded}"
            ),
            Self::Signer(error) => write!(f, "{error}"),
            Self::Verifier(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
            Self::Log(error) => write!(f, "{error}"),
            Self::Publisher(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MessagingCommandError {}
