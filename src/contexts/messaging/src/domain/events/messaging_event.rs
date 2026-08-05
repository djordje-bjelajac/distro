use crate::domain::events::{
    MessageDeliveryStateChanged, MessageDuplicateIgnored, MessageGapClosed, MessageReceived,
    MessageRejected, MessageSent,
};

/// Everything the `messaging` context publishes (canvas §2.3).
///
/// The union exists so `EventPublisherPort` can be object-safe with one method
/// — a trait with a method per event could not be held behind `dyn` and would
/// grow a breaking change every time an event is added. Adding a variant here
/// makes every exhaustive consumer fail to compile, which is the intended
/// pressure.
///
/// All six are **context-internal**. Unlike `membership`, this context
/// publishes no cross-context contract: it consumes `PeerConnected` /
/// `PeerDisconnected` from `shared_types` and tells no other context anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagingEvent {
    MessageSent(MessageSent),
    MessageReceived(MessageReceived),
    MessageRejected(MessageRejected),
    MessageDuplicateIgnored(MessageDuplicateIgnored),
    MessageGapClosed(MessageGapClosed),
    MessageDeliveryStateChanged(MessageDeliveryStateChanged),
}

impl From<MessageSent> for MessagingEvent {
    fn from(event: MessageSent) -> Self {
        Self::MessageSent(event)
    }
}

impl From<MessageReceived> for MessagingEvent {
    fn from(event: MessageReceived) -> Self {
        Self::MessageReceived(event)
    }
}

impl From<MessageRejected> for MessagingEvent {
    fn from(event: MessageRejected) -> Self {
        Self::MessageRejected(event)
    }
}

impl From<MessageDuplicateIgnored> for MessagingEvent {
    fn from(event: MessageDuplicateIgnored) -> Self {
        Self::MessageDuplicateIgnored(event)
    }
}

impl From<MessageGapClosed> for MessagingEvent {
    fn from(event: MessageGapClosed) -> Self {
        Self::MessageGapClosed(event)
    }
}

impl From<MessageDeliveryStateChanged> for MessagingEvent {
    fn from(event: MessageDeliveryStateChanged) -> Self {
        Self::MessageDeliveryStateChanged(event)
    }
}
