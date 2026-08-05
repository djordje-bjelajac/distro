use crate::domain::MessageId;

/// A message already applied arrived again and changed nothing (invariant 6).
///
/// This is the *normal* case, not an anomaly: gossip, relay paths and retries
/// all redeliver, and AC7 requires exactly-once application over at-least-once
/// delivery. The event exists so the count is visible in local diagnostics —
/// silence would make it impossible to tell healthy redelivery from a
/// duplication storm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageDuplicateIgnored {
    pub id: MessageId,
}
