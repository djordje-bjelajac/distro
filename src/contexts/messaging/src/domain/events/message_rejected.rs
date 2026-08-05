use shared_types::PeerId;

use crate::domain::events::RejectionReason;
use crate::domain::{ConversationId, SequenceNumber};

/// Inbound content was refused and never became a message (AC6, invariant 10).
///
/// # Why the author is only *claimed*
///
/// A message's author is the peer whose signature verified (invariant 4). Most
/// rejections happen before that is established — a forged envelope names
/// whoever the forger liked — so this event reports what the envelope asserted
/// and leaves the reader to treat it as a claim. For the one rejection the
/// domain itself produces
/// ([`ArrivedAfterGapClosed`](RejectionReason::ArrivedAfterGapClosed)) the
/// author has already been verified, but the field keeps its honest name rather
/// than changing meaning per variant.
///
/// `sequence` is `None` when the content never decoded far enough to have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageRejected {
    pub conversation: ConversationId,
    /// The author the envelope asserted — verified only for domain-produced
    /// rejections.
    pub claimed_author: PeerId,
    pub sequence: Option<SequenceNumber>,
    pub reason: RejectionReason,
}
