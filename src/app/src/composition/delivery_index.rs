use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use messaging::domain::MessageId;
use shared_types::EnvelopeSignature;

/// The root's map from an envelope signature to the message it carried.
///
/// # Why the correlation has to happen here
///
/// The network reports a direct message's fate by **signature**:
///
/// > *Correlated by envelope signature rather than by `MessageId`: a
/// > `MessageId` lives inside the payload, and this layer carries payloads
/// > unread. The root, which signed the envelope, knows which message the
/// > signature belongs to.*
/// > — `NetworkEvent::DirectMessageDelivered`
///
/// Both of `InboundEnvelopePort`'s outbound-fate methods —
/// `message_delivered` and `message_delivery_failed` — take a `MessageId`.
/// Nothing in either context holds both halves: `messaging` never sees a
/// signature after the signer produced it, and `infra-net-libp2p` never reads a
/// payload. The composition root is the only place the two meet, and this is
/// that place.
///
/// # What is not decided here
///
/// Nothing about delivery. This is a lookup table: the `MessageId` it returns
/// was assigned by the conversation aggregate when the message was composed,
/// and both transitions it enables are `Conversation`'s. The root does not
/// decide that a message arrived or that it never will — it reports what the
/// network said, and the conversation rules on whether the move is legal.
///
/// # Bounded, and an entry is consumed
///
/// A signature is answered at most once — delivered *or* failed, never both —
/// so [`take`](Self::take) removes it. That is what stops a late
/// `DirectMessageFailed` from overturning a message the user has already been
/// shown as delivered, before the conversation ever has to refuse it.
///
/// What is left behind is the messages nothing ever answered for, and that set
/// is capped: the oldest entries are evicted, because a peer that never
/// acknowledges must not be able to make this process hold one entry per
/// message forever (S6). An evicted entry costs a late report, which is counted
/// and stated rather than acted on — the alternative is guessing which message
/// it named.
#[derive(Debug)]
pub struct DeliveryIndex {
    entries: Mutex<Correlations>,
    capacity: usize,
}

#[derive(Debug, Default)]
struct Correlations {
    by_signature: HashMap<EnvelopeSignature, MessageId>,
    order: VecDeque<EnvelopeSignature>,
}

impl DeliveryIndex {
    /// Unanswered direct messages tracked at once.
    ///
    /// A human sends a few messages a minute and each is answered or failed
    /// within the request timeout, so this is only ever deep when a peer has
    /// gone quiet mid-conversation. 1024 covers that by orders of magnitude
    /// while bounding the memory an unresponsive peer can cost.
    pub const DEFAULT_CAPACITY: usize = 1024;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(Correlations::default()),
            capacity: capacity.max(1),
        }
    }

    /// Remembers which message a signature belongs to.
    pub fn record(&self, signature: EnvelopeSignature, message: MessageId) {
        let mut entries = self.lock();

        while entries.order.len() >= self.capacity {
            if let Some(oldest) = entries.order.pop_front() {
                entries.by_signature.remove(&oldest);
            }
        }

        if entries.by_signature.insert(signature, message).is_none() {
            entries.order.push_back(signature);
        }
    }

    /// Consumes the correlation for `signature`, if one is held.
    pub fn take(&self, signature: &EnvelopeSignature) -> Option<MessageId> {
        let mut entries = self.lock();

        let message = entries.by_signature.remove(signature)?;
        entries.order.retain(|held| held != signature);

        Some(message)
    }

    /// How many messages are still waiting for an answer.
    pub fn outstanding(&self) -> usize {
        self.lock().by_signature.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Correlations> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for DeliveryIndex {
    fn default() -> Self {
        Self::new()
    }
}
