use shared_types::Envelope;

use crate::domain::MessageId;
use crate::domain::events::{MessageDeliveryStateChanged, MessageGapClosed};
use crate::ports::{InboundVerdict, MessagingCommandError};

/// The **inbound** (driving) contract for everything the network reports about
/// messages (canvas §4, inbound column; S3).
///
/// This is the boundary S3 names. Wire data reaches the domain only through
/// [`accept_envelope`](Self::accept_envelope), after the size, signature,
/// version, and block checks; an adapter never reaches past it into a
/// conversation, and never constructs a domain aggregate from raw bytes.
///
/// # Why the gap sweep lives here
///
/// [`close_aged_gaps`](Self::close_aged_gaps) is the "and nothing arrived" half
/// of the same story. A gap means *not yet received* for a bounded interval
/// (invariant 5, rule R), and the interval elapsing is an observation about
/// inbound traffic — so it is evaluated at the same boundary, driven by the
/// same runtime tick. Putting it on the query side would make a redraw mutate
/// state; putting it on the send side would suggest a user asked for it.
///
/// **Nothing else drives that sweep.** Without a caller on a tick, a gap only
/// ever closes when the per-author buffer fills, which on a quiet conversation
/// may be never — the author would simply stop being heard (AC10, AC15).
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn InboundEnvelopePort + Send + Sync>`.
pub trait InboundEnvelopePort {
    /// Takes in one envelope that arrived from the network.
    ///
    /// The pipeline, in this order: protocol version (S2/AC14), signature
    /// (invariant 4/10 — an invalid one never reaches a read model), local
    /// block list (invariant 11), payload decode, then the conversation, with
    /// the arrival instant read from this peer's own clock and never from the
    /// author's claim.
    ///
    /// Every refusal is data, not an error: `Err` is reserved for a
    /// collaborator that could not do its job.
    fn accept_envelope(&self, envelope: Envelope) -> Result<InboundVerdict, MessagingCommandError>;

    /// The recipient acknowledged a 1:1 message this peer sent (AC11).
    ///
    /// A report, which is why it is here rather than on
    /// [`SendMessagePort`](crate::ports::SendMessagePort): the acknowledgement
    /// comes from the network, not from the user.
    fn message_delivered(
        &self,
        id: MessageId,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError>;

    /// Gives up on every gap that has stayed open past the tolerance window,
    /// and makes the runs waiting behind them visible (rule R, AC15).
    ///
    /// Driven by a clock tick from the runtime. Idempotent: calling it with
    /// nothing aged does nothing and reports nothing, so a fast tick costs
    /// only the sweep.
    fn close_aged_gaps(&self) -> Result<Vec<MessageGapClosed>, MessagingCommandError>;
}
