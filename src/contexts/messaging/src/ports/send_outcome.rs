use crate::domain::events::MessageSent;
use crate::domain::{DeliveryFailure, DeliveryState};

/// What became of a message this peer sent (AC11).
///
/// Two facts rather than one, because they answer different questions:
/// [`sent`](Self::sent) identifies the message that now exists locally, and
/// [`delivery`](Self::delivery) says what is known about its journey.
///
/// # Why a failed send is a success here
///
/// A direct send the transport refused returns `Ok` with
/// [`DeliveryState::Failed`]. AC11 makes silent loss a non-state, so the
/// message must exist, be visible, and carry a reason the user can act on —
/// and an `Err` would discard exactly that record. The error case is reserved
/// for a send that produced *no* message at all.
///
/// A broadcast is always [`Published`](DeliveryState::Published): gossip has no
/// recipient set and no acknowledgement, so there is nothing else this peer
/// could honestly claim (D3, AC10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendOutcome {
    /// The message that now exists in this peer's conversation.
    pub sent: MessageSent,
    /// What is known about its delivery at the moment the send returned.
    pub delivery: DeliveryState,
}

impl SendOutcome {
    /// Whether the message is awaiting acknowledgement.
    pub const fn is_pending(&self) -> bool {
        self.delivery.is_pending()
    }

    /// Why it will not arrive, when it will not; `None` otherwise.
    pub const fn failure_reason(&self) -> Option<DeliveryFailure> {
        self.delivery.failure_reason()
    }
}
