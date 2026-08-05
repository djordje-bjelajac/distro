use std::sync::Arc;

use messaging::domain::{ConversationId, MessageId};
use messaging::ports::{MessagePayload, MessageTransportError, MessageTransportPort};
use shared_types::{Envelope, PeerId};

use crate::composition::DeliveryIndex;

/// `messaging`'s `MessageTransportPort`, wrapped so the root can recognise the
/// acknowledgement that comes back later.
///
/// # Why the correlation is taken here and not from the send's result
///
/// `SendMessagePort::send_direct` returns a `SendOutcome` carrying a
/// `MessageSent { id, claimed_sent_at }` — and no signature, because a context
/// that models conversations has no reason to hold one. The network, meanwhile,
/// reports delivery by signature and nothing else. The only moment both facts
/// exist in one place is the instant a signed envelope is handed to the
/// transport, which is this call.
///
/// # What is reconstructed, and why that is not a decision
///
/// The `MessageId` is rebuilt from three things this peer already established:
///
/// * `envelope.author` — this peer, since it signed it (invariant 4);
/// * `ConversationId::Direct(to)` — the address the caller gave;
/// * the sequence number inside the payload, read with `MessagePayload::decode`
///   — the *same* `messaging::ports` type that wrote it moments earlier.
///
/// Nothing is chosen. The conversation aggregate assigned that sequence, and
/// this reads it back out of the bytes it was written into. A hand-rolled
/// parse, or a guess, would be a second definition of a message's identity; the
/// port's own codec is the only one there is.
///
/// # Broadcasts are not indexed
///
/// Gossip has no recipient and no acknowledgement (D3, AC10), so a broadcast
/// has nothing to correlate and an entry for one would sit in the index until
/// it was evicted. `publish_broadcast` therefore delegates untouched.
///
/// # A payload this build cannot read is not a send failure
///
/// If the payload does not decode, the envelope is still sent: it is a
/// well-formed signed envelope and the recipient may well read it. What is lost
/// is the ability to mark it delivered, which leaves it `Pending` — visible,
/// not silent. Refusing the send instead would turn a correlation problem into
/// a delivery failure, which is a worse lie.
pub struct CorrelatingTransport {
    inner: Arc<dyn MessageTransportPort + Send + Sync>,
    deliveries: Arc<DeliveryIndex>,
}

impl CorrelatingTransport {
    /// Wraps `inner`, recording every direct send in `deliveries`.
    pub const fn new(
        inner: Arc<dyn MessageTransportPort + Send + Sync>,
        deliveries: Arc<DeliveryIndex>,
    ) -> Self {
        Self { inner, deliveries }
    }

    /// The identifier of the message this envelope carries to `to`, or `None`
    /// when the payload is not one this build can read.
    fn message_of(to: PeerId, envelope: &Envelope) -> Option<MessageId> {
        let payload = MessagePayload::decode(&envelope.payload).ok()?;

        Some(MessageId::new(
            envelope.author,
            ConversationId::Direct(to),
            payload.sequence(),
        ))
    }
}

impl MessageTransportPort for CorrelatingTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        // Recorded before the send, not after: the acknowledgement can arrive
        // on the driver's thread while this one is still returning, and an
        // index written afterwards would miss it.
        if let Some(message) = Self::message_of(to, envelope) {
            self.deliveries.record(envelope.signature, message);
        }

        self.inner.send_direct(to, envelope)
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        self.inner.publish_broadcast(envelope)
    }
}

impl std::fmt::Debug for CorrelatingTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CorrelatingTransport")
            .field("outstanding", &self.deliveries.outstanding())
            .finish_non_exhaustive()
    }
}
