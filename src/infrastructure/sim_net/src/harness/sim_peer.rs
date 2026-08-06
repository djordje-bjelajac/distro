use std::sync::Arc;

use identity::application::IdentityContext;
use identity::domain::events::{PeerBlocked, PeerUnblocked, PeerVerified};
use identity::ports::{
    IdentityCommandPort, IdentityKeyStoreError, IdentityQueryPort, LocalIdentityAssumption,
    LocalIdentitySummary, PeerTrustCommandError, PeerTrustState, TrustRecordStoreError,
    TrustRecordStorePort,
};
use membership::application::{MembershipContext, MembershipSettings};
use membership::domain::events::PeerPresenceExpired;
use membership::domain::{Endpoint, JoinTicket, NetworkStatus, SessionOutcome};
use membership::ports::{
    ClockPort as MembershipClockPort, DiscoveredPeer, DiscoveryOutcome,
    EventPublisherError as MembershipPublisherError, EventPublisherPort as MembershipPublisherPort,
    ForgetPeersError, ForgetPeersOutcome, InboundSessionPort, JoinNetworkPort, JoinOutcome,
    KnownPeerView, LeaveOutcome, MembershipCommandError, MembershipQueryPort, NetworkView,
    PeerCachePort, PeerDiscoveryPort, PeerTransportPort,
};
use messaging::application::{MessagingContext, MessagingPorts, MessagingSettings};
use messaging::domain::events::{MessageDeliveryStateChanged, MessageGapClosed};
use messaging::domain::{
    ConversationId, DeliveryFailure, DeliveryState, Message, MessageBody, MessageId,
};
use messaging::ports::{
    AuthorPolicyPort, ClearHistoryPort, ClearedHistory, ClockPort as MessagingClockPort,
    EnvelopeSignerPort, EnvelopeVerifierPort, EventPublisherPort as MessagingPublisherPort,
    InboundEnvelopePort, InboundVerdict, MessageLogError, MessageLogPort, MessageTransportPort,
    MessagingCommandError, MessagingQueryPort, PeerLifecyclePort, SendMessagePort, SendOutcome,
    SequenceCounterPort,
};
use shared_types::{Envelope, PeerConnected, PeerDisconnected, PeerId};

use crate::clock::VirtualClock;
use crate::crypto::{SimKeyStore, SimSigner, SimVerifier};
use crate::fabric::{SimFabric, SimMessageTransport, SimPeerDiscovery, SimPeerTransport};
use crate::harness::{DurablePeerState, SimSettings};
use crate::stores::{InMemoryMessageLog, TrustRecordAuthorPolicy};
use crate::trace::{EventTrace, MembershipEventRecorder, MessagingEventRecorder};

/// One simulated peer: all three contexts, assembled the way a composition root
/// assembles them.
///
/// # What "the way a composition root assembles them" means here
///
/// Every cross-context wiring the canvas describes is made once, in
/// [`assemble`](SimPeer::assemble), and cannot be got wrong by a scenario:
///
/// * `identity`'s and `messaging`'s signer ports are wired to one [`SimSigner`]
///   over one keypair, and both verifier ports to one [`SimVerifier`] — so
///   invariant 4 holds in both directions with one key (canvas §4).
/// * `messaging`'s `AuthorPolicyPort` reads `identity`'s block list through
///   [`TrustRecordAuthorPolicy`], so blocking a peer through the identity
///   command port stops its envelopes at the messaging boundary (invariant 11).
/// * `membership`'s `PeerConnected` / `PeerDisconnected` are queued by this
///   peer's publisher and fanned into `messaging`'s `PeerLifecyclePort` by the
///   network's pump (D10) — the one seam between the two contexts, carrying a
///   `PeerId` and nothing else.
/// * Both contexts' clocks are the one [`VirtualClock`], so a roster ageing
///   presence and a conversation ageing a gap can never disagree.
///
/// # Nothing here starts anything
///
/// No timer, no task, no thread. The presence sweep and the gap sweep are
/// driven from outside through the inbound ports, exactly as both contexts'
/// docs require — which is what makes a scenario's control over time total.
pub struct SimPeer {
    peer: PeerId,
    label: String,
    durable: Arc<DurablePeerState>,
    membership_events: Arc<MembershipEventRecorder>,
    log: Arc<InMemoryMessageLog>,
    identity: IdentityContext,
    membership: MembershipContext,
    messaging: MessagingContext,
}

impl SimPeer {
    /// Assembles a peer's three contexts over the shared fabric, clock, and
    /// trace.
    ///
    /// The durable state is supplied rather than created, which is what makes a
    /// restart a rebuild of everything *except* the identity, the peer cache,
    /// the trust records, and the outbound sequence counter (D7, D12, AC16).
    pub fn assemble(
        label: &str,
        durable: Arc<DurablePeerState>,
        fabric: &Arc<SimFabric>,
        clock: &Arc<VirtualClock>,
        trace: &Arc<EventTrace>,
        settings: SimSettings,
    ) -> Self {
        let peer = durable.peer();

        let identity = IdentityContext::new(
            Arc::new(SimKeyStore::new(Arc::clone(durable.keypair()))),
            Arc::clone(durable.trust()) as Arc<dyn TrustRecordStorePort + Send + Sync>,
        );

        let membership_events = Arc::new(MembershipEventRecorder::new(
            peer,
            Arc::clone(clock),
            Arc::clone(trace),
        ));

        let membership = MembershipContext::new(
            MembershipSettings::for_local_peer(peer)
                .with_protocol(settings.protocol)
                .with_liveness_windows(settings.liveness_windows),
            Arc::clone(clock) as Arc<dyn MembershipClockPort + Send + Sync>,
            Arc::new(SimPeerTransport::new(peer, Arc::clone(fabric)))
                as Arc<dyn PeerTransportPort + Send + Sync>,
            Arc::new(SimPeerDiscovery::new(peer, Arc::clone(fabric)))
                as Arc<dyn PeerDiscoveryPort + Send + Sync>,
            Arc::clone(durable.cache()) as Arc<dyn PeerCachePort + Send + Sync>,
            Arc::clone(&membership_events) as Arc<dyn MembershipPublisherPort + Send + Sync>,
        );

        // One signer over one keypair, behind both contexts' signer ports.
        let signer = Arc::new(SimSigner::new(Arc::clone(durable.keypair())));
        let log = Arc::new(InMemoryMessageLog::with_capacity(
            settings.message_log_capacity,
        ));

        let messaging = MessagingContext::new(
            MessagingSettings::for_local_peer(peer)
                .speaking(settings.protocol)
                .with_gap_tolerance(settings.gap_tolerance),
            MessagingPorts {
                clock: Arc::clone(clock) as Arc<dyn MessagingClockPort + Send + Sync>,
                counter: Arc::clone(durable.counter())
                    as Arc<dyn SequenceCounterPort + Send + Sync>,
                signer: signer as Arc<dyn EnvelopeSignerPort + Send + Sync>,
                verifier: Arc::new(SimVerifier) as Arc<dyn EnvelopeVerifierPort + Send + Sync>,
                policy: Arc::new(TrustRecordAuthorPolicy::new(Arc::clone(durable.trust())))
                    as Arc<dyn AuthorPolicyPort + Send + Sync>,
                transport: Arc::new(SimMessageTransport::new(peer, Arc::clone(fabric)))
                    as Arc<dyn MessageTransportPort + Send + Sync>,
                log: Arc::clone(&log) as Arc<dyn MessageLogPort + Send + Sync>,
                publisher: Arc::new(MessagingEventRecorder::new(
                    peer,
                    Arc::clone(clock),
                    Arc::clone(trace),
                )) as Arc<dyn MessagingPublisherPort + Send + Sync>,
            },
        );

        Self {
            peer,
            label: label.to_owned(),
            durable,
            membership_events,
            log,
            identity,
            membership,
            messaging,
        }
    }

    // ------------------------------------------------------------- identity

    /// This peer's stable identity (AC9).
    pub const fn id(&self) -> PeerId {
        self.peer
    }

    /// The name the scenario knows this peer by.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// What outlived this peer's last process (D12).
    pub fn durable(&self) -> &Arc<DurablePeerState> {
        &self.durable
    }

    // ----------------------------------------------------------- contexts

    /// The assembled `identity` context.
    pub const fn identity(&self) -> &IdentityContext {
        &self.identity
    }

    /// The assembled `membership` context.
    pub const fn membership(&self) -> &MembershipContext {
        &self.membership
    }

    /// The assembled `messaging` context.
    pub const fn messaging(&self) -> &MessagingContext {
        &self.messaging
    }

    /// This peer's in-memory message log — the mirror that dies with the
    /// process (D7).
    pub fn message_log(&self) -> &Arc<InMemoryMessageLog> {
        &self.log
    }

    /// The cross-context events waiting to be fanned into `messaging`.
    pub(crate) fn membership_events(&self) -> &Arc<MembershipEventRecorder> {
        &self.membership_events
    }

    // ------------------------------------------------- identity convenience

    /// Assumes this process's identity: the load-or-create every launch does
    /// (AC1, AC9).
    ///
    /// Idempotent — a second call reports `AlreadyAssumed` and writes nothing.
    pub fn initialize_identity(&self) -> Result<LocalIdentityAssumption, IdentityKeyStoreError> {
        self.identity.commands().initialize_local_identity(None)
    }

    /// What this peer shows as itself, or `None` before it has assumed an
    /// identity.
    pub fn local_identity(&self) -> Option<LocalIdentitySummary> {
        self.identity.queries().local_identity()
    }

    /// Records an out-of-band fingerprint confirmation for `peer` (D5).
    pub fn verify(&self, peer: PeerId) -> Result<Option<PeerVerified>, TrustRecordStoreError> {
        self.identity.commands().verify_peer(peer)
    }

    /// Blocks `peer` locally.
    ///
    /// Takes effect in `messaging` immediately: its author policy reads this
    /// same block list, so the next envelope from `peer` is refused with
    /// `AuthorBlocked` (invariant 11).
    pub fn block(&self, peer: PeerId) -> Result<PeerBlocked, PeerTrustCommandError> {
        self.identity.commands().block_peer(peer)
    }

    /// Unblocks `peer`, which returns to the verification state it kept
    /// throughout.
    pub fn unblock(&self, peer: PeerId) -> Result<PeerUnblocked, PeerTrustCommandError> {
        self.identity.commands().unblock_peer(peer)
    }

    /// What this peer locally believes about `peer`.
    pub fn trust_state(&self, peer: PeerId) -> Result<PeerTrustState, TrustRecordStoreError> {
        self.identity.queries().peer_trust_state(peer)
    }

    /// Every peer this one is dropping traffic from.
    pub fn blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        self.identity.queries().blocked_peers()
    }

    // ----------------------------------------------- membership convenience

    /// Walks the D1 bootstrap ladder with no ticket: cached peers, then the
    /// LAN.
    pub fn join(&self) -> Result<JoinOutcome, MembershipPublisherError> {
        self.membership.join().join_network(None)
    }

    /// Walks the ladder with a ticket to fall back on, used only if the two
    /// free rungs produce nothing.
    pub fn join_with_ticket(
        &self,
        ticket: JoinTicket,
    ) -> Result<JoinOutcome, MembershipPublisherError> {
        self.membership.join().join_network(Some(ticket))
    }

    /// Closes every session, saves the peer cache, and announces the departure.
    pub fn leave(&self) -> Result<LeaveOutcome, MembershipPublisherError> {
        self.membership.join().leave_network()
    }

    /// Dials a peer this instance already knows — the UI's "connect" action.
    pub fn connect_to(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.membership.join().connect_to_peer(peer)
    }

    /// Leaves, then forgets every known peer and empties the cache, so the
    /// next start is a cold one (canvas `0013`).
    pub fn forget_peers(&self) -> Result<ForgetPeersOutcome, ForgetPeersError> {
        self.membership.join().forget_known_peers()
    }

    /// Throws away every conversation this instance holds, keeping its
    /// identity and its outbound sequence marks (canvas `0013`).
    pub fn clear_history(&self) -> Result<ClearedHistory, MessageLogError> {
        self.messaging.history().clear_history()
    }

    /// Re-derives every peer's presence against the clock and announces those
    /// that have newly fallen silent (AC5).
    pub fn expire_presence(&self) -> Result<Vec<PeerPresenceExpired>, MembershipPublisherError> {
        self.membership.sessions().expire_presence()
    }

    /// Reports that a discovery mechanism saw `discovered` — a claim, never a
    /// fact, until the handshake proves it.
    pub fn peer_observed(
        &self,
        discovered: DiscoveredPeer,
    ) -> Result<DiscoveryOutcome, MembershipCommandError> {
        self.membership.sessions().peer_observed(discovered)
    }

    /// Reports that a remote dialled this peer at `endpoints`.
    pub fn session_opened(
        &self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
    ) -> Result<SessionOutcome, MembershipCommandError> {
        self.membership.sessions().session_opened(peer, endpoints)
    }

    /// Reports that the authenticated handshake with `peer` completed — the
    /// only moment `PeerConnected` is published.
    pub fn session_established(
        &self,
        peer: PeerId,
    ) -> Result<SessionOutcome, MembershipCommandError> {
        self.membership.sessions().session_established(peer)
    }

    /// Reports that the link to `peer` ended.
    pub fn session_closed(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.membership.sessions().session_closed(peer)
    }

    /// Reports evidence of life from `peer` (invariant 7).
    pub fn peer_heartbeat(&self, peer: PeerId) -> Result<(), MembershipCommandError> {
        self.membership.sessions().peer_heartbeat(peer)
    }

    /// The status line and the roster rows as one snapshot, taken at one
    /// instant through one classification (canvas `0010` D5).
    ///
    /// What a screen showing both should call, and therefore what a scenario
    /// asserting on both should call: assembling the two from
    /// [`network_status`](Self::network_status) and
    /// [`known_peers`](Self::known_peers) would let a test pass while the two
    /// readings contradicted each other, which is the defect the query exists
    /// to make unrepresentable.
    pub fn network_view(&self) -> NetworkView {
        self.membership.queries().network_view()
    }

    /// Every peer this instance knows about, in `PeerId` order, with presence
    /// derived at the moment of the call.
    pub fn known_peers(&self) -> Vec<KnownPeerView> {
        self.membership.queries().known_peers()
    }

    /// The peers whose evidence of life is fresh enough to be `Online`.
    pub fn online_peers(&self) -> Vec<PeerId> {
        self.membership.queries().online_peers()
    }

    /// How connected this instance currently is.
    pub fn network_status(&self) -> NetworkStatus {
        self.membership.queries().network_status()
    }

    /// Whether this peer holds an established session with `peer`.
    pub fn is_connected_to(&self, peer: PeerId) -> bool {
        self.known_peers()
            .iter()
            .any(|view| view.peer == peer && view.is_connected())
    }

    // ------------------------------------------------ messaging convenience

    /// Composes a 1:1 message to `to`.
    ///
    /// Returns `Ok` even when the transport refuses: the message exists and its
    /// outcome carries the `Failed(reason)` the user must be shown (AC11).
    ///
    /// # Panics
    ///
    /// If `text` is not an admissible [`MessageBody`] — empty, blank, or over
    /// 16 KiB. A scenario writing an inadmissible body has a bug in the
    /// scenario, and a `Result` there would be noise on every call site.
    pub fn send_direct(
        &self,
        to: PeerId,
        text: &str,
    ) -> Result<SendOutcome, MessagingCommandError> {
        self.messaging.send().send_direct(to, body(text))
    }

    /// Composes a message for the network-wide channel (D3).
    ///
    /// # Panics
    ///
    /// If `text` is not an admissible [`MessageBody`]; see
    /// [`send_direct`](Self::send_direct).
    pub fn publish_broadcast(&self, text: &str) -> Result<SendOutcome, MessagingCommandError> {
        self.messaging.send().publish_broadcast(body(text))
    }

    /// Hands one envelope straight to this peer's inbound boundary, bypassing
    /// the fabric.
    ///
    /// The way to stage an envelope no honest peer would send — a forged
    /// signature, a wrong major version, an unknown payload kind — without
    /// inventing a network condition to carry it (AC6, AC14).
    pub fn accept_envelope(
        &self,
        envelope: Envelope,
    ) -> Result<InboundVerdict, MessagingCommandError> {
        self.messaging.inbound().accept_envelope(envelope)
    }

    /// Records that the recipient acknowledged a 1:1 message this peer sent
    /// (AC11).
    pub fn message_delivered(
        &self,
        id: MessageId,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        self.messaging.inbound().message_delivered(id)
    }

    /// Records that a 1:1 message this peer sent will not arrive, and why
    /// (D10, AC11).
    ///
    /// # The report no frame can carry
    ///
    /// [`message_delivered`](Self::message_delivered) rides a frame back from
    /// the recipient, so the fabric drives it. This one has no such carrier: a
    /// transport answers `send_direct` with `Ok` as soon as it has *queued* the
    /// request, and a refusal or timeout surfaces afterwards as a local network
    /// event — from this peer's own transport, about a message the far side may
    /// never have seen. A scenario plays that report here, exactly as the
    /// composition root plays a `DirectMessageFailed`, which is what keeps a
    /// refused message from sitting `Pending` for the life of a session that is
    /// still perfectly healthy.
    ///
    /// The [`DeliveryFailure`] is the caller's to state — it is what was
    /// observed, and a defaulted one would be a guess (AC11). Whether the move
    /// is legal is the conversation's ruling: an unknown message, one already
    /// delivered or already failed, and any broadcast message come back as
    /// typed errors rather than panics or silent overwrites.
    pub fn message_delivery_failed(
        &self,
        id: MessageId,
        reason: DeliveryFailure,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        self.messaging.inbound().message_delivery_failed(id, reason)
    }

    /// Gives up on every gap that has stayed open past the tolerance window
    /// (rule R, AC15).
    pub fn close_aged_gaps(&self) -> Result<Vec<MessageGapClosed>, MessagingCommandError> {
        self.messaging.inbound().close_aged_gaps()
    }

    /// Every conversation this peer has recorded history for.
    pub fn conversations(&self) -> Vec<ConversationId> {
        self.messaging
            .queries()
            .conversations()
            .expect("the in-memory log is always readable")
    }

    /// One conversation's applied messages — never the buffered ones
    /// (invariant 5).
    pub fn history(&self, conversation: ConversationId) -> Vec<Message> {
        self.messaging.queries().history(conversation)
    }

    /// The broadcast channel's applied messages.
    pub fn broadcast_history(&self) -> Vec<Message> {
        self.history(ConversationId::Broadcast)
    }

    /// The 1:1 conversation with `peer`, as this peer sees it.
    pub fn direct_history(&self, peer: PeerId) -> Vec<Message> {
        self.history(ConversationId::Direct(peer))
    }

    /// The bodies of one conversation, in the order the read model returns
    /// them — the shape most assertions actually want.
    pub fn transcript(&self, conversation: ConversationId) -> Vec<String> {
        self.history(conversation)
            .iter()
            .map(|message| message.body().to_string())
            .collect()
    }

    /// What is known about one message's delivery (AC11).
    pub fn delivery_state(&self, id: MessageId) -> Option<DeliveryState> {
        self.messaging.queries().delivery_state(id)
    }

    /// Fans one `PeerConnected` into `messaging` (D10).
    pub(crate) fn peer_connected(&self, event: PeerConnected) -> Result<(), MessagingCommandError> {
        self.messaging.lifecycle().peer_connected(event)
    }

    /// Fans one `PeerDisconnected` into `messaging`, failing that peer's
    /// pending directs (D10, AC11).
    pub(crate) fn peer_disconnected(
        &self,
        event: PeerDisconnected,
    ) -> Result<Vec<MessageDeliveryStateChanged>, MessagingCommandError> {
        self.messaging.lifecycle().peer_disconnected(event)
    }
}

/// Builds a [`MessageBody`], stating plainly that an inadmissible one is a
/// scenario bug rather than a network condition.
fn body(text: &str) -> MessageBody {
    MessageBody::new(text).unwrap_or_else(|error| {
        panic!("a scenario composed an inadmissible message body ({error}): {text:?}")
    })
}
