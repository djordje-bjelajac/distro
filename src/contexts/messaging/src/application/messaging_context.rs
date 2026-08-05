use std::sync::Arc;

use crate::application::commands::{
    InboundEnvelopeService, OutboundComposer, PeerLifecycleService, SendMessageService,
};
use crate::application::queries::MessagingQueryService;
use crate::application::{
    ConversationRegistry, MessageRecorder, MessagingPorts, MessagingSettings,
};

/// The assembled `messaging` context: its four inbound ports, wired over the
/// outbound ports a composition root supplies.
///
/// # Why all four are built together
///
/// CQRS separates the command and query *paths*, not the state they describe.
/// The three command services and the query service must see one
/// [`ConversationRegistry`], or a message just accepted from the network would
/// be invisible in the pane rendering that conversation — a defect that
/// surfaces only at runtime, as a message that never appears. Constructing them
/// here makes that mistake unrepresentable at the root: there is no way to hand
/// them different registries.
///
/// # What OP-12 wires
///
/// The root supplies `infra-net-libp2p`'s [`MessageTransportPort`],
/// `infra-store-fs`'s [`SequenceCounterPort`] and in-memory
/// [`MessageLogPort`] (D7, D12), one [`ClockPort`], an
/// [`EventPublisherPort`], and — the two cross-context wirings, neither of
/// which is an import — the one underlying signer and verifier behind
/// [`EnvelopeSignerPort`] / [`EnvelopeVerifierPort`], and `identity`'s block
/// list behind [`AuthorPolicyPort`].
///
/// It then drives the context through [`send`](Self::send) as
/// `&dyn SendMessagePort` (the composer), [`inbound`](Self::inbound) as
/// `&dyn InboundEnvelopePort` (the network pump **and a clock tick**),
/// [`lifecycle`](Self::lifecycle) as `&dyn PeerLifecyclePort` (fanning
/// `membership`'s `PeerConnected` / `PeerDisconnected` in), and
/// [`queries`](Self::queries) as `&dyn MessagingQueryPort` (every redraw).
///
/// Nothing here starts a task, opens a socket, or reads a clock: the context is
/// inert until it is called. In particular **no timer is started** — the gap
/// sweep is driven from outside through `InboundEnvelopePort`, which is what
/// keeps every test in this crate free of real time (AC13) and what makes the
/// sweep a stated obligation of the root rather than a hidden thread.
pub struct MessagingContext {
    send: SendMessageService,
    inbound: InboundEnvelopeService,
    lifecycle: PeerLifecycleService,
    queries: MessagingQueryService,
}

impl MessagingContext {
    /// Assembles all four inbound ports over the given outbound ports.
    pub fn new(settings: MessagingSettings, ports: MessagingPorts) -> Self {
        let registry = Arc::new(ConversationRegistry::for_local_peer(
            settings.local_peer,
            Arc::clone(&ports.counter),
        ));
        // Built once and shared: the composer is what keeps the counter and the
        // conversations in step, and the recorder is what keeps the log and the
        // event stream written in one order.
        let composer = OutboundComposer::new(
            Arc::clone(&registry),
            settings,
            Arc::clone(&ports.clock),
            ports.counter,
            ports.signer,
        );
        let recorder = MessageRecorder::new(Arc::clone(&ports.log), ports.publisher);

        Self {
            send: SendMessageService::new(
                Arc::clone(&registry),
                composer,
                ports.transport,
                recorder.clone(),
            ),
            inbound: InboundEnvelopeService::new(
                Arc::clone(&registry),
                settings,
                ports.clock,
                ports.verifier,
                ports.policy,
                recorder.clone(),
            ),
            lifecycle: PeerLifecycleService::new(Arc::clone(&registry), recorder),
            queries: MessagingQueryService::new(registry, ports.log),
        }
    }

    /// The inbound port for composing: direct and broadcast.
    pub const fn send(&self) -> &SendMessageService {
        &self.send
    }

    /// The inbound port for reports: arriving envelopes, acknowledgements, and
    /// the gap sweep.
    pub const fn inbound(&self) -> &InboundEnvelopeService {
        &self.inbound
    }

    /// The inbound port for peer lifecycle news fanned in from `membership`.
    pub const fn lifecycle(&self) -> &PeerLifecycleService {
        &self.lifecycle
    }

    /// The inbound port for reads. Nothing behind it writes.
    pub const fn queries(&self) -> &MessagingQueryService {
        &self.queries
    }

    /// Splits the context so a root can hand each side to a different owner —
    /// the network pump, the UI task, the gap ticker — while all four keep the
    /// shared registry this constructor established.
    pub fn into_parts(
        self,
    ) -> (
        SendMessageService,
        InboundEnvelopeService,
        PeerLifecycleService,
        MessagingQueryService,
    ) {
        (self.send, self.inbound, self.lifecycle, self.queries)
    }
}
