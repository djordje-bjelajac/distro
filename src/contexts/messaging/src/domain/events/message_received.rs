use crate::domain::{MessageId, Millis};

/// A remote message became part of the conversation and is now visible.
///
/// Raised on *application*, not on arrival. A message that arrived out of order
/// waits in the buffer and raises this only when the run leading to it closes
/// (invariant 5), so a consumer of these events sees each author's messages in
/// that author's send order (AC8) and never sees content twice (invariant 6).
///
/// The `id`'s author is the peer whose signature verified, never a payload
/// field (invariant 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageReceived {
    pub id: MessageId,
    /// The instant the *author* claimed as the send time — display only, and
    /// not to be trusted: it is another peer's clock, freely falsifiable.
    pub claimed_sent_at: Millis,
}
