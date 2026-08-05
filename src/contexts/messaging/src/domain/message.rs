use shared_types::PeerId;

use crate::domain::events::MessageDeliveryStateChanged;
use crate::domain::{
    ConversationId, DeliveryFailure, DeliveryState, DeliveryStateError, MessageBody, MessageId,
    Millis, SequenceNumber,
};

/// One text message in one conversation (canvas §2.3).
///
/// # The instant is a claim, not a fact
///
/// [`claimed_sent_at`](Self::claimed_sent_at) is what the *author* said the
/// send time was. It is kept for display and used for nothing else: it is
/// another peer's clock, unsynchronised with this one and freely falsifiable,
/// so no ordering, expiry, or dedup rule in this context reads it. Ordering is
/// [`SequenceNumber`]'s job (invariant 5, AC8).
///
/// # Direction decides the delivery lifecycle
///
/// [`outbound`](Self::outbound) and [`received`](Self::received) are separate
/// constructors because the initial [`DeliveryState`] follows from both the
/// conversation and the direction, and deriving it here is what keeps a
/// received message from ever sitting at `Pending` — it plainly arrived — and a
/// broadcast from ever claiming a delivery nobody can confirm (D3, AC10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    id: MessageId,
    body: MessageBody,
    claimed_sent_at: Millis,
    delivery: DeliveryState,
}

impl Message {
    /// A message this peer composed, about to travel.
    pub fn outbound(id: MessageId, body: MessageBody, claimed_sent_at: Millis) -> Self {
        let delivery = match id.conversation() {
            ConversationId::Broadcast => DeliveryState::Published,
            ConversationId::Direct(_) => DeliveryState::Pending,
        };

        Self {
            id,
            body,
            claimed_sent_at,
            delivery,
        }
    }

    /// A message from another peer that reached this one.
    ///
    /// A direct message is [`Delivered`](DeliveryState::Delivered) on
    /// construction: it is in hand, so "pending" would be false.
    pub fn received(id: MessageId, body: MessageBody, claimed_sent_at: Millis) -> Self {
        let delivery = match id.conversation() {
            ConversationId::Broadcast => DeliveryState::Published,
            ConversationId::Direct(_) => DeliveryState::Delivered,
        };

        Self {
            id,
            body,
            claimed_sent_at,
            delivery,
        }
    }

    pub const fn id(&self) -> MessageId {
        self.id
    }

    /// The peer whose signature verified on this message's envelope
    /// (invariant 4).
    pub const fn author(&self) -> PeerId {
        self.id.author()
    }

    pub const fn conversation(&self) -> ConversationId {
        self.id.conversation()
    }

    pub const fn sequence(&self) -> SequenceNumber {
        self.id.sequence()
    }

    pub const fn body(&self) -> &MessageBody {
        &self.body
    }

    /// The author's claimed send time. Display only — see the type docs.
    pub const fn claimed_sent_at(&self) -> Millis {
        self.claimed_sent_at
    }

    pub const fn delivery_state(&self) -> DeliveryState {
        self.delivery
    }

    /// Records delivery, reporting the transition.
    ///
    /// Reachable only through [`Conversation`](crate::domain::Conversation):
    /// the aggregate root is the single place state changes, so no caller can
    /// mutate a message the root still believes is pending.
    pub(super) fn mark_delivered(
        &mut self,
    ) -> Result<MessageDeliveryStateChanged, DeliveryStateError> {
        self.transition(self.delivery.mark_delivered()?)
    }

    /// Records failure with its stated reason, reporting the transition.
    pub(super) fn mark_failed(
        &mut self,
        reason: DeliveryFailure,
    ) -> Result<MessageDeliveryStateChanged, DeliveryStateError> {
        self.transition(self.delivery.mark_failed(reason)?)
    }

    fn transition(
        &mut self,
        to: DeliveryState,
    ) -> Result<MessageDeliveryStateChanged, DeliveryStateError> {
        let from = self.delivery;
        self.delivery = to;

        Ok(MessageDeliveryStateChanged {
            id: self.id,
            from,
            to,
        })
    }
}
