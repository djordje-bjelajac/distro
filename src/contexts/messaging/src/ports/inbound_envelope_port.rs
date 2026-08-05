use shared_types::Envelope;

use crate::domain::events::{MessageDeliveryStateChanged, MessageGapClosed};
use crate::domain::{DeliveryFailure, MessageId};
use crate::ports::{InboundVerdict, MessagingCommandError};

/// The **inbound** (driving) contract for everything the network reports about
/// messages (canvas §4, inbound column; S3).
///
/// This is the boundary S3 names. Wire data reaches the domain only through
/// [`accept_envelope`](Self::accept_envelope), after the size, signature,
/// version, and block checks; an adapter never reaches past it into a
/// conversation, and never constructs a domain aggregate from raw bytes.
///
/// # The four methods, and which direction each one serves
///
/// Nothing here is a decision — every method reports something that already
/// happened out on the network. The decisions are
/// [`SendMessagePort`](crate::ports::SendMessagePort)'s.
///
/// | Method | Direction | Reports |
/// | --- | --- | --- |
/// | [`accept_envelope`](Self::accept_envelope) | **inbound traffic** | a message another peer sent *to* this one arrived |
/// | [`message_delivered`](Self::message_delivered) | **outbound fate** | a message *this* peer sent was acknowledged by its recipient |
/// | [`message_delivery_failed`](Self::message_delivery_failed) | **outbound fate** | a message *this* peer sent will not arrive, and why |
/// | [`close_aged_gaps`](Self::close_aged_gaps) | **inbound absence** | a tolerance window elapsed and nothing filled it |
///
/// The two middle rows are one message's two possible endings, and they are
/// separate methods because they run in the same direction but say opposite
/// things. Confusing them for each other is the mistake this table exists to
/// prevent: an acknowledgement cannot express a refusal, and no amount of
/// inbound traffic can.
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

    /// One 1:1 message this peer sent will not arrive, for the stated reason
    /// (D10, AC11).
    ///
    /// # Why this exists as a method of its own
    ///
    /// A transport answers `send_direct` with `Ok` once it has *queued* the
    /// request; a refusal or a timeout comes back later, as an event. By then
    /// the send has returned, and while the session is still up no other method
    /// can move that message off `Pending`:
    ///
    /// * [`message_delivered`](Self::message_delivered) is the opposite
    ///   ending, and reporting one to mean the other is a lie to the user.
    /// * [`peer_disconnected`](crate::ports::PeerLifecyclePort::peer_disconnected)
    ///   fails *every* pending direct to that peer, which is both too much for
    ///   one refused message and unavailable while the link is healthy.
    /// * [`accept_envelope`](Self::accept_envelope) runs the other way — it
    ///   carries what other peers said, not what became of what this one sent.
    ///
    /// So without this method the message sits `Pending` for the life of the
    /// session, which is the silent loss AC11 declares a non-state. A caller
    /// that has no reason to report cannot cause the gap; a caller that has one
    /// and nowhere to send it can, which is why it is stated here rather than
    /// inferred.
    ///
    /// # The reason is the caller's, the transition is the domain's
    ///
    /// The [`DeliveryFailure`] passed in is what was actually observed —
    /// AC11 requires a cause the user can act on, and a defaulted one would be
    /// a guess. Whether the move is legal at all is the conversation's ruling:
    /// a message already delivered or already failed, and a broadcast message
    /// in any state, come back as a typed
    /// [`ConversationError`](crate::domain::ConversationError) rather than
    /// overwriting what the user has already been shown.
    fn message_delivery_failed(
        &self,
        id: MessageId,
        reason: DeliveryFailure,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError>;

    /// Gives up on every gap that has stayed open past the tolerance window,
    /// and makes the runs waiting behind them visible (rule R, AC15).
    ///
    /// Driven by a clock tick from the runtime. Idempotent: calling it with
    /// nothing aged does nothing and reports nothing, so a fast tick costs
    /// only the sweep.
    fn close_aged_gaps(&self) -> Result<Vec<MessageGapClosed>, MessagingCommandError>;
}
