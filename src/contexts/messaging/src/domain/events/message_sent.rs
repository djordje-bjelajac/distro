use crate::domain::{MessageId, Millis};

/// The local peer added a message of its own to a conversation.
///
/// Raised the moment the message is appended and assigned its sequence number,
/// not when it reaches anyone: whether it arrived is
/// [`MessageDeliveryStateChanged`](crate::domain::events::MessageDeliveryStateChanged)'s
/// business, and for a broadcast nobody can say (D3, AC10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageSent {
    pub id: MessageId,
    /// The instant this peer claimed as the send time — display only.
    pub claimed_sent_at: Millis,
}
