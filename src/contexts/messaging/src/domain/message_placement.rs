use crate::domain::events::{MessageDuplicateIgnored, MessageRejected};
use crate::domain::{MessageId, SequenceNumber};

/// Where one inbound message ended up (invariant 5, rule R; invariant 6).
///
/// Four placements, and none of them is "dropped". That is the point: a gap
/// means not yet received rather than lost, a duplicate changes nothing, an
/// abandoned range is reported, and AC11/AC15 make silent loss a non-state in
/// both directions. An enum that must be matched is how those become impossible
/// to ignore at the call site.
///
/// This says what happened to *this* message only. What became visible — after
/// a drain usually more than one message, after an abandoned gap possibly
/// many — is carried alongside it by
/// [`InboundOutcome`](crate::domain::InboundOutcome), because messages can
/// become visible in the same breath as this one is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessagePlacement {
    /// It became part of the conversation.
    Applied(MessageId),
    /// A gap precedes it, so it is held until the gap closes or is abandoned.
    Buffered {
        id: MessageId,
        /// The sequence number the conversation is missing.
        awaiting: SequenceNumber,
    },
    /// Already applied or already held: a no-op (invariant 6, AC7).
    DuplicateIgnored(MessageDuplicateIgnored),
    /// Refused with a stated reason; nothing was stored and nothing already
    /// held was evicted.
    Rejected(MessageRejected),
}
