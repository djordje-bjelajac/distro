use shared_types::PeerId;

use crate::domain::events::GapCloseCause;
use crate::domain::{ConversationId, SequenceNumber};

/// One author's log gave up on a run of sequence numbers and moved past it
/// (invariant 5, rule R; AC15).
///
/// This is the inbound mirror of
/// [`MessageDeliveryStateChanged`](crate::domain::events::MessageDeliveryStateChanged):
/// outbound, AC11 says silent loss is not a state; inbound, AC15 says the same
/// about content that never arrived. Skipping quietly would leave a user
/// reading a conversation with a hole in it and no way to know.
///
/// # The range is what was abandoned, not what was received
///
/// [`from`](Self::from)..=[`to`](Self::to) is inclusive and names the sequence
/// numbers this peer has decided it will never display: everything the author
/// numbered in that span is gone as far as this peer is concerned. A one-message
/// gap has `from == to`. Anything arriving in the range afterwards is
/// [`RejectionReason::ArrivedAfterGapClosed`](crate::domain::events::RejectionReason::ArrivedAfterGapClosed),
/// never a duplicate.
///
/// The messages the close *released* are not carried here — they are in the
/// conversation, in the author's order, from `to + 1` onwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageGapClosed {
    pub conversation: ConversationId,
    /// The author whose run was cut. Verified: only content that already
    /// reached a log can open a gap in it (invariant 4).
    pub author: PeerId,
    /// The first abandoned sequence number.
    pub from: SequenceNumber,
    /// The last abandoned sequence number, inclusive.
    pub to: SequenceNumber,
    pub cause: GapCloseCause,
}
