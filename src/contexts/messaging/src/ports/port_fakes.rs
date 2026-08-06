//! Deterministic fakes for this context's outbound ports.
//!
//! Test-only (`#[cfg(test)]`) and never linked into a binary. Domain and
//! application tests must touch no network, clock, filesystem, or external
//! service (AC13), so every collaborator these tests need is implemented here
//! in memory, with no threads, no randomness, and no I/O.
//!
//! Interior mutability rather than `&mut self` is deliberate: every port takes
//! `&self`, so a fake that recorded its calls through a mutable borrow would
//! not implement the trait a real adapter must. It is `Mutex`/atomics rather
//! than `Cell`/`RefCell` because the application layer holds its ports as
//! `Arc<dyn …Port + Send + Sync>` — the shape a composition root needs. The
//! locking is uncontended in tests and never a source of nondeterminism.
//!
//! The signature scheme below is **not cryptography** and must never leave
//! tests: it is a deterministic keyed digest standing in for Ed25519. It gives
//! what the tests actually need — a signature that depends on both the signing
//! peer and the exact bytes signed, so tampering and wrong-signer cases are
//! detectable — without this crate depending on a crypto library.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use shared_types::{Envelope, EnvelopeSignature, PeerId};

use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, Message, Millis, SequenceNumber};
use crate::ports::{
    AuthorPolicyPort, ClockPort, EnvelopeSignerError, EnvelopeSignerPort, EnvelopeVerifierError,
    EnvelopeVerifierPort, EventPublisherError, EventPublisherPort, MessageLogError, MessageLogPort,
    MessageTransportError, MessageTransportPort, SequenceCounterError, SequenceCounterPort,
    SignatureVerdict, UnsignedEnvelope,
};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Reads a fake's lock without panicking on a poisoned mutex: a fake that
/// failed an assertion in one test must not turn every later test into a panic
/// with a misleading cause.
fn guard<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A deterministic stand-in for `sign(peer_secret_key, message)`.
///
/// Keyed by the signing peer's public bytes, which in a fake is equivalent to
/// keying by its secret: it makes each peer's signatures distinct and lets the
/// matching verifier recompute them from the envelope's author.
pub(crate) fn fake_signature(signer: &PeerId, message: &[u8]) -> EnvelopeSignature {
    let mut bytes = [0u8; EnvelopeSignature::LENGTH];

    for (block, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hash = FNV_OFFSET_BASIS ^ block as u64;
        for byte in signer.as_bytes().iter().chain(message) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        chunk.copy_from_slice(&hash.to_be_bytes());
    }

    EnvelopeSignature::new(bytes)
}

/// A signer holding one peer's key that records the exact bytes it was asked
/// to sign.
pub(crate) struct RecordingSigner {
    peer: PeerId,
    signed_inputs: Mutex<Vec<Vec<u8>>>,
}

impl RecordingSigner {
    pub(crate) const fn holding_key_of(peer: PeerId) -> Self {
        Self {
            peer,
            signed_inputs: Mutex::new(Vec::new()),
        }
    }

    /// Every byte string handed to [`EnvelopeSignerPort::sign`], in order.
    pub(crate) fn signed_inputs(&self) -> Vec<Vec<u8>> {
        guard(&self.signed_inputs).clone()
    }
}

impl EnvelopeSignerPort for RecordingSigner {
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        if unsigned.author() != self.peer {
            return Err(EnvelopeSignerError::AuthorMismatch);
        }

        let message = unsigned.signable_bytes();
        let signature = fake_signature(&self.peer, &message);
        guard(&self.signed_inputs).push(message);
        Ok(signature)
    }
}

/// A signer that always fails with a given typed error.
pub(crate) struct FailingSigner(pub(crate) EnvelopeSignerError);

impl EnvelopeSignerPort for FailingSigner {
    fn sign(&self, _unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        Err(self.0)
    }
}

/// The verifier matching [`RecordingSigner`]: recomputes the expected signature
/// from the envelope's own author and signable bytes.
pub(crate) struct CheckingVerifier;

impl EnvelopeVerifierPort for CheckingVerifier {
    fn verify(&self, envelope: &Envelope) -> Result<SignatureVerdict, EnvelopeVerifierError> {
        let expected = fake_signature(&envelope.author, &envelope.signable_bytes());

        Ok(if expected == envelope.signature {
            SignatureVerdict::Valid
        } else {
            SignatureVerdict::Invalid
        })
    }
}

/// A verifier that cannot perform the check at all.
pub(crate) struct UnavailableVerifier;

impl EnvelopeVerifierPort for UnavailableVerifier {
    fn verify(&self, _envelope: &Envelope) -> Result<SignatureVerdict, EnvelopeVerifierError> {
        Err(EnvelopeVerifierError::VerifierUnavailable)
    }
}

/// A transport that keeps everything it was handed, in order.
#[derive(Default)]
pub(crate) struct RecordingTransport {
    direct: Mutex<Vec<(PeerId, Envelope)>>,
    broadcast: Mutex<Vec<Envelope>>,
}

impl RecordingTransport {
    /// Every direct send, as `(recipient, envelope)` — the recipient is a
    /// `PeerId` because that is the only address this context has.
    pub(crate) fn sent_direct(&self) -> Vec<(PeerId, Envelope)> {
        guard(&self.direct).clone()
    }

    pub(crate) fn published(&self) -> Vec<Envelope> {
        guard(&self.broadcast).clone()
    }
}

impl MessageTransportPort for RecordingTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        guard(&self.direct).push((to, envelope.clone()));
        Ok(())
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        guard(&self.broadcast).push(envelope.clone());
        Ok(())
    }
}

/// A transport that always fails with a given typed error.
pub(crate) struct FailingTransport(pub(crate) MessageTransportError);

impl MessageTransportPort for FailingTransport {
    fn send_direct(&self, _to: PeerId, _envelope: &Envelope) -> Result<(), MessageTransportError> {
        Err(self.0)
    }

    fn publish_broadcast(&self, _envelope: &Envelope) -> Result<(), MessageTransportError> {
        Err(self.0)
    }
}

/// An in-memory message log, keyed by conversation in a deterministic order.
#[derive(Default)]
pub(crate) struct InMemoryMessageLog {
    entries: Mutex<BTreeMap<ConversationId, Vec<Message>>>,
}

impl MessageLogPort for InMemoryMessageLog {
    fn append(&self, message: &Message) -> Result<(), MessageLogError> {
        guard(&self.entries)
            .entry(message.conversation())
            .or_default()
            .push(message.clone());
        Ok(())
    }

    fn load(&self, conversation: ConversationId) -> Result<Vec<Message>, MessageLogError> {
        Ok(guard(&self.entries)
            .get(&conversation)
            .cloned()
            .unwrap_or_default())
    }

    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError> {
        Ok(guard(&self.entries).keys().copied().collect())
    }

    fn clear(&self) -> Result<usize, MessageLogError> {
        let mut entries = guard(&self.entries);
        let dropped = entries.values().map(Vec::len).sum();
        entries.clear();
        Ok(dropped)
    }
}

/// A log that cannot be reached at all.
pub(crate) struct UnavailableMessageLog;

impl MessageLogPort for UnavailableMessageLog {
    fn append(&self, _message: &Message) -> Result<(), MessageLogError> {
        Err(MessageLogError::Unavailable)
    }

    fn load(&self, _conversation: ConversationId) -> Result<Vec<Message>, MessageLogError> {
        Err(MessageLogError::Unavailable)
    }

    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError> {
        Err(MessageLogError::Unavailable)
    }

    fn clear(&self) -> Result<usize, MessageLogError> {
        Err(MessageLogError::Unavailable)
    }
}

/// A clock that stands still until a test moves it.
pub(crate) struct FixedClock(Mutex<Millis>);

impl FixedClock {
    pub(crate) const fn at(instant: Millis) -> Self {
        Self(Mutex::new(instant))
    }

    /// Moves time forward by `millis`. Forward only — the port's contract is
    /// that readings never go backwards.
    pub(crate) fn advance(&self, millis: u64) {
        let mut now = guard(&self.0);
        *now = Millis::from_millis(now.as_millis().saturating_add(millis));
    }
}

impl ClockPort for FixedClock {
    fn now(&self) -> Millis {
        *guard(&self.0)
    }
}

/// A publisher that keeps every event, in order.
#[derive(Default)]
pub(crate) struct RecordingEventPublisher {
    events: Mutex<Vec<MessagingEvent>>,
}

impl RecordingEventPublisher {
    pub(crate) fn published(&self) -> Vec<MessagingEvent> {
        guard(&self.events).clone()
    }
}

impl EventPublisherPort for RecordingEventPublisher {
    fn publish(&self, event: MessagingEvent) -> Result<(), EventPublisherError> {
        guard(&self.events).push(event);
        Ok(())
    }
}

/// A publisher that cannot accept events.
pub(crate) struct UnavailableEventPublisher;

impl EventPublisherPort for UnavailableEventPublisher {
    fn publish(&self, _event: MessagingEvent) -> Result<(), EventPublisherError> {
        Err(EventPublisherError::Unavailable)
    }
}

/// A local block list holding exactly the peers a test named (invariant 11).
#[derive(Default)]
pub(crate) struct LocalBlockList {
    blocked: BTreeSet<PeerId>,
}

impl LocalBlockList {
    pub(crate) fn blocking(peers: impl IntoIterator<Item = PeerId>) -> Self {
        Self {
            blocked: peers.into_iter().collect(),
        }
    }
}

impl AuthorPolicyPort for LocalBlockList {
    fn is_blocked(&self, peer: PeerId) -> bool {
        self.blocked.contains(&peer)
    }
}

/// An outbound sequence counter kept in memory, with the store it would have
/// persisted to modelled as its starting state.
///
/// [`restored_with`](Self::restored_with) is how a test spells "this peer has
/// restarted": the process is new, the counter is not (D12).
#[derive(Default)]
pub(crate) struct InMemorySequenceCounter {
    issued: Mutex<BTreeMap<ConversationId, SequenceNumber>>,
}

impl InMemorySequenceCounter {
    pub(crate) fn restored_with(
        entries: impl IntoIterator<Item = (ConversationId, SequenceNumber)>,
    ) -> Self {
        Self {
            issued: Mutex::new(entries.into_iter().collect()),
        }
    }
}

impl SequenceCounterPort for InMemorySequenceCounter {
    fn issue_next(
        &self,
        conversation: ConversationId,
    ) -> Result<SequenceNumber, SequenceCounterError> {
        let mut issued = guard(&self.issued);

        let next = match issued.get(&conversation) {
            None => SequenceNumber::FIRST,
            Some(last) => last
                .successor()
                .map_err(|_| SequenceCounterError::Exhausted)?,
        };
        // Recorded before it is returned, as the port's contract requires.
        issued.insert(conversation, next);

        Ok(next)
    }

    fn last_issued(
        &self,
        conversation: ConversationId,
    ) -> Result<Option<SequenceNumber>, SequenceCounterError> {
        Ok(guard(&self.issued).get(&conversation).copied())
    }
}

/// A counter store that cannot be reached at all.
pub(crate) struct UnavailableSequenceCounter;

impl SequenceCounterPort for UnavailableSequenceCounter {
    fn issue_next(
        &self,
        _conversation: ConversationId,
    ) -> Result<SequenceNumber, SequenceCounterError> {
        Err(SequenceCounterError::Unavailable)
    }

    fn last_issued(
        &self,
        _conversation: ConversationId,
    ) -> Result<Option<SequenceNumber>, SequenceCounterError> {
        Err(SequenceCounterError::Unavailable)
    }
}
