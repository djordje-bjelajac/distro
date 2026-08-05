use std::fmt;

use crate::domain::DeliveryFailure;

/// What is known about a message's delivery (canvas §2.3, D10, AC11).
///
/// Two disjoint lifecycles share one type because a message has exactly one
/// delivery state whichever conversation it is in:
///
/// - **Direct** messages start [`Pending`](Self::Pending) and end
///   [`Delivered`](Self::Delivered) or [`Failed`](Self::Failed) — the failure
///   always naming a [`DeliveryFailure`]. There is no third ending: AC11 makes
///   silent loss a non-state, so a message that stops being pending has told
///   the user which of the two happened.
/// - **Broadcast** messages are [`Published`](Self::Published) and stay there.
///   Gossip has no recipient set and no acknowledgement (D3, AC10), so
///   "delivered" would be a claim this peer cannot make. Publishing is the
///   whole of what it knows.
///
/// Which lifecycle a message enters is decided once, by
/// [`Message`](crate::domain::Message), from its conversation and its
/// direction — this type only guards the moves.
///
/// Every terminal state is terminal: re-marking is a typed rejection rather
/// than a silent overwrite, so a late acknowledgement can never resurrect a
/// message the user was already told had failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryState {
    /// Direct only: handed to the transport, not yet acknowledged.
    Pending,
    /// Direct only: the recipient has it.
    Delivered,
    /// Direct only: it will not arrive, for the stated reason.
    Failed(DeliveryFailure),
    /// Broadcast only: released to the gossip topic.
    Published,
}

impl DeliveryState {
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Whether no further transition is possible.
    pub const fn is_terminal(&self) -> bool {
        !self.is_pending()
    }

    /// The stated cause, when this message failed.
    pub const fn failure_reason(&self) -> Option<DeliveryFailure> {
        match self {
            Self::Failed(reason) => Some(*reason),
            _ => None,
        }
    }

    /// Records that the recipient acknowledged the message.
    pub const fn mark_delivered(self) -> Result<Self, DeliveryStateError> {
        match self {
            Self::Pending => Ok(Self::Delivered),
            from => Err(DeliveryStateError::InvalidTransition {
                from,
                to: Self::Delivered,
            }),
        }
    }

    /// Records that the message will not arrive, and why.
    pub const fn mark_failed(self, reason: DeliveryFailure) -> Result<Self, DeliveryStateError> {
        match self {
            Self::Pending => Ok(Self::Failed(reason)),
            from => Err(DeliveryStateError::InvalidTransition {
                from,
                to: Self::Failed(reason),
            }),
        }
    }
}

impl fmt::Display for DeliveryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Delivered => f.write_str("delivered"),
            Self::Failed(reason) => write!(f, "failed: {reason}"),
            Self::Published => f.write_str("published"),
        }
    }
}

/// Typed rejection of a [`DeliveryState`] transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStateError {
    /// The state machine has no such move; both ends are named so a
    /// diagnostic can say what was attempted.
    InvalidTransition {
        from: DeliveryState,
        to: DeliveryState,
    },
}

impl fmt::Display for DeliveryStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(f, "delivery state cannot move from {from} to {to}")
            }
        }
    }
}

impl std::error::Error for DeliveryStateError {}
