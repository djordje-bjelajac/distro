//! A `MessagingContext` assembled entirely from in-memory fakes.
//!
//! Every application test needs the same nine collaborators, and eight of them
//! are the same eight every time. Spelling that out per test file would bury
//! the one substitution each test is actually about — a transport that refuses,
//! a verifier that cannot run, a counter that survived a restart — in eight
//! lines of ceremony.
//!
//! Nothing here touches a network, a clock, a filesystem, or an external
//! service (AC13). The clock stands still until a test moves it by hand, so
//! every time-dependent rule is decided by the test rather than by how long it
//! took to run (S5).

use std::sync::Arc;

use shared_types::{Envelope, PayloadKind, PeerId, ProtocolVersion};

use crate::application::{MessagingContext, MessagingPorts, MessagingSettings};
use crate::domain::events::MessagingEvent;
use crate::domain::{ConversationId, Message, MessageBody, Millis, SequenceNumber};
use crate::ports::port_fakes::{
    CheckingVerifier, InMemoryMessageLog, InMemorySequenceCounter, LocalBlockList,
    RecordingEventPublisher, RecordingSigner, RecordingTransport,
};
use crate::ports::{
    AuthorPolicyPort, ClockPort, EnvelopeSignerPort, EnvelopeVerifierPort, EventPublisherPort,
    InboundVerdict, MessageLogPort, MessagePayload, MessageTransportPort, MessagingCommandError,
    MessagingQueryPort, SendMessagePort, SendOutcome, SequenceCounterPort, UnsignedEnvelope,
};

/// Where the fake clock starts. Far from zero so a test can tell an instant
/// this peer read from one an author invented.
pub(crate) const NOW: Millis = Millis::from_millis(1_000_000);

/// A clock this crate's tests move by hand.
pub(crate) struct TestClock(std::sync::Mutex<Millis>);

impl TestClock {
    fn starting_at(now: Millis) -> Self {
        Self(std::sync::Mutex::new(now))
    }

    /// Moves time forward by `millis`. Forward only — the port's contract is
    /// that readings never go backwards.
    pub(crate) fn advance(&self, millis: u64) {
        let mut now = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *now = Millis::from_millis(now.as_millis().saturating_add(millis));
    }
}

impl ClockPort for TestClock {
    fn now(&self) -> Millis {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Builds a context, substituting only the collaborators a test cares about.
pub(crate) struct TestContextBuilder {
    settings: MessagingSettings,
    clock: Arc<TestClock>,
    counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
    verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
    policy: Arc<dyn AuthorPolicyPort + Send + Sync>,
    transport: Arc<dyn MessageTransportPort + Send + Sync>,
    log: Arc<InMemoryMessageLog>,
    publisher: Arc<RecordingEventPublisher>,
}

impl TestContextBuilder {
    /// A context for `local`, with every collaborator working normally.
    pub(crate) fn for_local_peer(local: PeerId) -> Self {
        Self {
            settings: MessagingSettings::for_local_peer(local),
            clock: Arc::new(TestClock::starting_at(NOW)),
            counter: Arc::new(InMemorySequenceCounter::default()),
            signer: Arc::new(RecordingSigner::holding_key_of(local)),
            verifier: Arc::new(CheckingVerifier),
            policy: Arc::new(LocalBlockList::default()),
            transport: Arc::new(RecordingTransport::default()),
            log: Arc::new(InMemoryMessageLog::default()),
            publisher: Arc::new(RecordingEventPublisher::default()),
        }
    }

    pub(crate) fn with_settings(mut self, settings: MessagingSettings) -> Self {
        self.settings = settings;
        self
    }

    /// A counter of the test's own — one that already holds what a previous
    /// process issued (D12), or one that cannot be reached at all.
    ///
    /// The test keeps its own handle to it, which is why the built context does
    /// not hold one: a fake nobody asserts against is a field nobody needs.
    pub(crate) fn with_counter(
        mut self,
        counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    ) -> Self {
        self.counter = counter;
        self
    }

    pub(crate) fn with_signer(mut self, signer: Arc<dyn EnvelopeSignerPort + Send + Sync>) -> Self {
        self.signer = signer;
        self
    }

    pub(crate) fn with_verifier(
        mut self,
        verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
    ) -> Self {
        self.verifier = verifier;
        self
    }

    pub(crate) fn blocking(mut self, peers: impl IntoIterator<Item = PeerId>) -> Self {
        self.policy = Arc::new(LocalBlockList::blocking(peers));
        self
    }

    pub(crate) fn with_transport(
        mut self,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
    ) -> Self {
        self.transport = transport;
        self
    }

    pub(crate) fn build(self) -> TestContext {
        let context = MessagingContext::new(
            self.settings,
            MessagingPorts {
                clock: Arc::clone(&self.clock) as Arc<dyn ClockPort + Send + Sync>,
                counter: self.counter,
                signer: self.signer,
                verifier: self.verifier,
                policy: self.policy,
                transport: self.transport,
                log: Arc::clone(&self.log) as Arc<dyn MessageLogPort + Send + Sync>,
                publisher: Arc::clone(&self.publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
            },
        );

        TestContext {
            context,
            clock: self.clock,
            log: self.log,
            publisher: self.publisher,
        }
    }
}

/// An assembled context plus the fakes a test asserts against.
pub(crate) struct TestContext {
    pub(crate) context: MessagingContext,
    pub(crate) clock: Arc<TestClock>,
    pub(crate) log: Arc<InMemoryMessageLog>,
    pub(crate) publisher: Arc<RecordingEventPublisher>,
}

impl TestContext {
    pub(crate) fn send_direct(
        &self,
        to: PeerId,
        text: &str,
    ) -> Result<SendOutcome, MessagingCommandError> {
        self.context.send().send_direct(to, body(text))
    }

    pub(crate) fn publish_broadcast(
        &self,
        text: &str,
    ) -> Result<SendOutcome, MessagingCommandError> {
        self.context.send().publish_broadcast(body(text))
    }

    pub(crate) fn accept(
        &self,
        envelope: Envelope,
    ) -> Result<InboundVerdict, MessagingCommandError> {
        use crate::ports::InboundEnvelopePort;

        self.context.inbound().accept_envelope(envelope)
    }

    pub(crate) fn history(&self, conversation: ConversationId) -> Vec<Message> {
        self.context.queries().history(conversation)
    }

    /// The bodies visible in one conversation, in the order a reader sees them.
    pub(crate) fn visible_text(&self, conversation: ConversationId) -> Vec<String> {
        self.history(conversation)
            .iter()
            .map(|message| message.body().as_str().to_owned())
            .collect()
    }

    pub(crate) fn events(&self) -> Vec<MessagingEvent> {
        self.publisher.published()
    }

    /// What the message log mirror holds for one conversation — the durable
    /// half, deliberately distinct from what a query returns.
    pub(crate) fn mirrored(&self, conversation: ConversationId) -> Vec<Message> {
        self.log
            .load(conversation)
            .expect("the in-memory log answers")
    }
}

/// A body a test can rely on being admissible.
pub(crate) fn body(text: &str) -> MessageBody {
    MessageBody::new(text).expect("test bodies are within the size limits")
}

pub(crate) fn sequence(value: u64) -> SequenceNumber {
    SequenceNumber::new(value).expect("test sequence numbers are non-zero")
}

/// An envelope another peer signed with its own key, as one would arrive from
/// the network.
pub(crate) fn envelope_from(
    author: PeerId,
    kind: PayloadKind,
    payload: MessagePayload,
) -> Envelope {
    envelope_versioned(author, kind, payload, ProtocolVersion::CURRENT)
}

/// The same, speaking a stated protocol version (AC14).
pub(crate) fn envelope_versioned(
    author: PeerId,
    kind: PayloadKind,
    payload: MessagePayload,
    version: ProtocolVersion,
) -> Envelope {
    RecordingSigner::holding_key_of(author)
        .seal(UnsignedEnvelope::draft(
            author,
            version,
            kind,
            payload.encode(),
        ))
        .expect("the fake signer holds the author's key")
}

/// A direct message from `author` to this peer.
pub(crate) fn direct_from(author: PeerId, seq: u64, text: &str, claimed_at: Millis) -> Envelope {
    envelope_from(
        author,
        PayloadKind::DirectMessage,
        MessagePayload::new(sequence(seq), claimed_at, body(text)),
    )
}

/// A broadcast message from `author`.
pub(crate) fn broadcast_from(author: PeerId, seq: u64, text: &str, claimed_at: Millis) -> Envelope {
    envelope_from(
        author,
        PayloadKind::BroadcastMessage,
        MessagePayload::new(sequence(seq), claimed_at, body(text)),
    )
}
