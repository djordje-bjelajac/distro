use std::sync::{Arc, Mutex};

use infra_net_libp2p::swarm::{DirectMessageFailure, NetworkEvent, Reachability};
use membership::domain::events::PeerPresenceExpired;
use membership::domain::{Endpoint, JoinTicket, SessionOutcome};
use membership::ports::{
    DiscoveredPeer, DiscoveryOutcome, EventPublisherError, InboundSessionPort,
    MembershipCommandError, PeerDiscoveryError, PeerDiscoveryPort,
};
use messaging::domain::events::{MessageDeliveryStateChanged, MessageGapClosed};
use messaging::domain::{
    ConversationId, DeliveryFailure, DeliveryState, MessageId, MessagePlacement, SequenceNumber,
};
use messaging::ports::{InboundEnvelopePort, InboundVerdict, MessagingCommandError};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::composition::{DeliveryIndex, Diagnostics, LocalEndpoints, NoticeFeed};
use crate::runtime::EventRouter;
use crate::test_peers::{alice, bob};

/// Everything the router called, in order — the whole point of the type under
/// test is *which* calls it makes and in what sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Observed(PeerId),
    Opened(PeerId, Vec<Endpoint>),
    Established(PeerId),
    Closed(PeerId),
    Heartbeat(PeerId),
    Accepted(PeerId),
    Delivered(MessageId),
    Failed(MessageId, DeliveryFailure),
    Announced(Vec<Endpoint>),
}

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<Call>>,
    heartbeat_refuses: Mutex<bool>,
    /// The conversation's ruling, when a scenario wants one: a broadcast has
    /// no failed state, and a message already delivered keeps what the user was
    /// shown.
    delivery_failure_refusal: Mutex<Option<MessagingCommandError>>,
}

impl Recorder {
    fn push(&self, call: Call) {
        self.calls.lock().expect("no panic").push(call);
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("no panic").clone()
    }
}

impl InboundSessionPort for Recorder {
    fn peer_observed(
        &self,
        discovered: DiscoveredPeer,
    ) -> Result<DiscoveryOutcome, MembershipCommandError> {
        self.push(Call::Observed(discovered.peer));
        Ok(DiscoveryOutcome::Refreshed)
    }

    fn session_opened(
        &self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
    ) -> Result<SessionOutcome, MembershipCommandError> {
        self.push(Call::Opened(peer, endpoints));
        Ok(SessionOutcome::quiet())
    }

    fn session_established(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.push(Call::Established(peer));
        Ok(SessionOutcome::quiet())
    }

    fn session_closed(&self, peer: PeerId) -> Result<SessionOutcome, MembershipCommandError> {
        self.push(Call::Closed(peer));
        Ok(SessionOutcome::quiet())
    }

    fn peer_heartbeat(&self, peer: PeerId) -> Result<(), MembershipCommandError> {
        self.push(Call::Heartbeat(peer));

        if *self.heartbeat_refuses.lock().expect("no panic") {
            return Err(MembershipCommandError::Roster(
                membership::domain::PeerRosterError::UnknownPeer,
            ));
        }
        Ok(())
    }

    fn expire_presence(&self) -> Result<Vec<PeerPresenceExpired>, EventPublisherError> {
        Ok(Vec::new())
    }
}

impl InboundEnvelopePort for Recorder {
    fn accept_envelope(&self, envelope: Envelope) -> Result<InboundVerdict, MessagingCommandError> {
        self.push(Call::Accepted(envelope.author));

        Ok(InboundVerdict::Judged(MessagePlacement::Applied(
            MessageId::new(
                envelope.author,
                ConversationId::Broadcast,
                SequenceNumber::FIRST,
            ),
        )))
    }

    fn message_delivered(
        &self,
        id: MessageId,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        self.push(Call::Delivered(id));

        Ok(MessageDeliveryStateChanged {
            id,
            from: DeliveryState::Pending,
            to: DeliveryState::Delivered,
        })
    }

    fn message_delivery_failed(
        &self,
        id: MessageId,
        reason: DeliveryFailure,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        self.push(Call::Failed(id, reason));

        if let Some(error) = *self.delivery_failure_refusal.lock().expect("no panic") {
            return Err(error);
        }

        Ok(MessageDeliveryStateChanged {
            id,
            from: DeliveryState::Pending,
            to: DeliveryState::Failed(reason),
        })
    }

    fn close_aged_gaps(&self) -> Result<Vec<MessageGapClosed>, MessagingCommandError> {
        Ok(Vec::new())
    }
}

impl PeerDiscoveryPort for Recorder {
    fn announce(&self, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        self.push(Call::Announced(endpoints.to_vec()));
        Ok(())
    }

    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        Ok(Vec::new())
    }

    fn redeem_join_ticket(
        &self,
        _ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        Err(PeerDiscoveryError::TicketUnreachable)
    }
}

struct Harness {
    recorder: Arc<Recorder>,
    endpoints: Arc<LocalEndpoints>,
    deliveries: Arc<DeliveryIndex>,
    diagnostics: Arc<Diagnostics>,
    notices: Arc<NoticeFeed>,
    router: EventRouter,
}

fn harness() -> Harness {
    let recorder = Arc::new(Recorder::default());
    let endpoints = Arc::new(LocalEndpoints::new());
    let deliveries = Arc::new(DeliveryIndex::new());
    let diagnostics = Arc::new(Diagnostics::default());
    let notices = Arc::new(NoticeFeed::new());

    let router = EventRouter::new(
        Arc::clone(&recorder) as Arc<_>,
        Arc::clone(&recorder) as Arc<_>,
        Arc::clone(&recorder) as Arc<_>,
        Arc::clone(&endpoints),
        Arc::clone(&deliveries),
        Arc::clone(&diagnostics),
        Arc::clone(&notices),
    );

    Harness {
        recorder,
        endpoints,
        deliveries,
        diagnostics,
        notices,
        router,
    }
}

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("a valid address")
}

fn envelope(author: PeerId) -> Envelope {
    Envelope {
        version: ProtocolVersion::CURRENT,
        kind: PayloadKind::BroadcastMessage,
        author,
        payload: Vec::new(),
        signature: EnvelopeSignature::new([1; EnvelopeSignature::LENGTH]),
    }
}

#[test]
fn a_listening_endpoint_is_remembered_and_announces_nothing() {
    // `ListeningOn` maps to no port call: what a peer announces is what
    // `PeerTransportPort::listen` returned, and `membership` announces it.
    let harness = harness();

    harness
        .router
        .route(NetworkEvent::ListeningOn(endpoint("/ip4/10.0.0.1/tcp/1")));

    assert!(harness.recorder.calls().is_empty());
    assert_eq!(
        harness.endpoints.all(),
        vec![endpoint("/ip4/10.0.0.1/tcp/1")]
    );
}

#[test]
fn a_confirmed_external_address_re_announces_every_known_endpoint() {
    // The first moment a NAT-ed peer has a truthful address to publish — and
    // an announcement replaces rather than appends, so the whole set goes.
    let harness = harness();
    harness
        .router
        .route(NetworkEvent::ListeningOn(endpoint("/ip4/10.0.0.1/tcp/1")));

    harness
        .router
        .route(NetworkEvent::ExternalAddressConfirmed(endpoint(
            "/ip4/203.0.113.7/tcp/1",
        )));

    assert_eq!(
        harness.recorder.calls(),
        vec![Call::Announced(vec![
            endpoint("/ip4/203.0.113.7/tcp/1"),
            endpoint("/ip4/10.0.0.1/tcp/1"),
        ])]
    );
}

#[test]
fn a_confirmation_of_a_supplied_address_reports_the_override_as_in_effect() {
    // D6's second source. What the operator asked for is known at startup; that
    // it took hold is known only here, and the router is the one place both
    // could be joined up.
    let harness = harness();
    harness
        .diagnostics
        .record_supplied_external_addresses(&["/ip4/203.0.113.7/tcp/1".to_owned()]);

    harness
        .router
        .route(NetworkEvent::ExternalAddressConfirmed(endpoint(
            "/ip4/203.0.113.7/tcp/1",
        )));

    assert_eq!(
        harness.diagnostics.external_addresses_in_effect(),
        vec!["/ip4/203.0.113.7/tcp/1".to_owned()]
    );
}

#[test]
fn a_confirmation_no_option_asked_for_leaves_the_override_report_empty() {
    // The same event carries a corroborated observation and a successful probe.
    // Reporting either as an override would say "the flag took effect" to a
    // user who passed no flag (D6).
    let harness = harness();

    harness
        .router
        .route(NetworkEvent::ExternalAddressConfirmed(endpoint(
            "/ip4/203.0.113.7/tcp/1",
        )));

    assert!(
        harness
            .diagnostics
            .external_addresses_in_effect()
            .is_empty()
    );
    assert_eq!(
        harness.endpoints.all(),
        vec![endpoint("/ip4/203.0.113.7/tcp/1")],
        "the address is still advertised — only the override report is untouched"
    );
}

#[test]
fn the_same_confirmation_twice_announces_once() {
    let harness = harness();
    let confirmed = NetworkEvent::ExternalAddressConfirmed(endpoint("/ip4/203.0.113.7/tcp/1"));

    harness.router.route(confirmed.clone());
    harness.router.route(confirmed);

    assert_eq!(harness.recorder.calls().len(), 1);
}

#[test]
fn a_discovered_peer_is_reported_as_observed() {
    let harness = harness();

    harness
        .router
        .route(NetworkEvent::PeerDiscovered(DiscoveredPeer {
            peer: bob(),
            endpoints: vec![endpoint("/ip4/10.0.0.2/tcp/1")],
        }));

    assert_eq!(harness.recorder.calls(), vec![Call::Observed(bob())]);
}

#[test]
fn an_established_session_is_opened_then_established_in_that_order() {
    // The roster has a `Connecting` state a session must pass through, and the
    // endpoint the link runs on is only in the first call's arguments.
    let harness = harness();

    harness.router.route(NetworkEvent::SessionEstablished {
        peer: bob(),
        endpoint: endpoint("/ip4/10.0.0.2/tcp/1"),
    });

    assert_eq!(
        harness.recorder.calls(),
        vec![
            Call::Opened(bob(), vec![endpoint("/ip4/10.0.0.2/tcp/1")]),
            Call::Established(bob()),
        ]
    );
}

#[test]
fn a_closed_session_is_reported() {
    let harness = harness();

    harness
        .router
        .route(NetworkEvent::SessionClosed { peer: bob() });

    assert_eq!(harness.recorder.calls(), vec![Call::Closed(bob())]);
}

#[test]
fn an_arriving_envelope_is_evidence_of_life_before_it_is_judged() {
    // Order matters: an envelope from a blocked author still proves its
    // *carrier* is alive, and the carrier is not the author.
    let harness = harness();

    harness.router.route(NetworkEvent::EnvelopeReceived {
        from: bob(),
        envelope: envelope(alice()),
    });

    assert_eq!(
        harness.recorder.calls(),
        vec![Call::Heartbeat(bob()), Call::Accepted(alice())]
    );
}

#[test]
fn a_heartbeat_for_an_unknown_peer_does_not_stop_the_envelope() {
    // The ordinary case for a gossip relay this peer has never dialled: the
    // roster has no entry to refresh, and the envelope is still content.
    let harness = harness();
    *harness.recorder.heartbeat_refuses.lock().expect("no panic") = true;

    harness.router.route(NetworkEvent::EnvelopeReceived {
        from: bob(),
        envelope: envelope(alice()),
    });

    assert_eq!(
        harness.recorder.calls(),
        vec![Call::Heartbeat(bob()), Call::Accepted(alice())]
    );
}

#[test]
fn an_applied_envelope_is_counted() {
    let harness = harness();

    harness.router.route(NetworkEvent::EnvelopeReceived {
        from: bob(),
        envelope: envelope(alice()),
    });

    assert_eq!(harness.diagnostics.envelopes_accepted(), 1);
}

#[test]
fn an_acknowledged_direct_message_is_marked_delivered_by_signature() {
    // The network reports by signature and the port takes a `MessageId`; the
    // root is the only place both halves exist (AC11).
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    let message = MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::FIRST,
    );
    harness.deliveries.record(signature, message);

    harness.router.route(NetworkEvent::DirectMessageDelivered {
        peer: bob(),
        signature,
    });

    assert_eq!(harness.recorder.calls(), vec![Call::Delivered(message)]);
}

#[test]
fn an_acknowledgement_for_an_unknown_signature_is_counted_and_nothing_is_marked() {
    // Evicted, or already answered. There is no message this could name, and
    // guessing at one would mark the wrong message delivered.
    let harness = harness();

    harness.router.route(NetworkEvent::DirectMessageDelivered {
        peer: bob(),
        signature: EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]),
    });

    assert!(harness.recorder.calls().is_empty());
    assert_eq!(harness.diagnostics.uncorrelated_reports(), 1);
}

#[test]
fn a_signature_is_answered_once_even_if_the_network_reports_twice() {
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    harness.deliveries.record(
        signature,
        MessageId::new(
            alice(),
            ConversationId::Direct(bob()),
            SequenceNumber::FIRST,
        ),
    );

    harness.router.route(NetworkEvent::DirectMessageDelivered {
        peer: bob(),
        signature,
    });
    harness.router.route(NetworkEvent::DirectMessageDelivered {
        peer: bob(),
        signature,
    });

    assert_eq!(harness.recorder.calls().len(), 1);
}

#[test]
fn a_failed_direct_message_reaches_failed_while_the_session_stays_up() {
    // The only path that can move one message off `Pending` without closing
    // the link: `message_delivered` is the opposite ending, and
    // `peer_disconnected` fails every pending direct to the peer.
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    let message = MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::FIRST,
    );
    harness.deliveries.record(signature, message);

    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature,
        reason: DirectMessageFailure::NotAcknowledged,
    });

    assert_eq!(
        harness.recorder.calls(),
        vec![Call::Failed(message, DeliveryFailure::RetriesExhausted)]
    );
    // No session was closed and no other peer was touched: a refused message
    // is news about one message, not about a link.
    assert!(
        !harness
            .recorder
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Closed(_))),
    );
    assert_eq!(harness.diagnostics.direct_delivery_failures(), 1);
}

#[test]
fn the_transport_reason_survives_into_the_notice() {
    // The delivery state is one of five domain reasons; the sentence beside it
    // is the diagnosis the state cannot carry (AC11: a cause a user can act
    // on).
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    harness.deliveries.record(
        signature,
        MessageId::new(
            alice(),
            ConversationId::Direct(bob()),
            SequenceNumber::FIRST,
        ),
    );

    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature,
        reason: DirectMessageFailure::Refused,
    });

    let notices = harness.notices.all();
    assert!(
        notices.iter().any(|notice| notice.text.contains("refused")),
        "a refusal that reads like a timeout is a lost diagnosis: {notices:?}"
    );
}

#[test]
fn failing_one_message_leaves_the_peers_other_pending_directs_alone() {
    // The distinction this method exists for: `peer_disconnected` fails every
    // pending direct to a peer, which is the wrong answer for one message the
    // recipient refused while the link is healthy.
    let harness = harness();
    let first = EnvelopeSignature::new([1; EnvelopeSignature::LENGTH]);
    let second = EnvelopeSignature::new([2; EnvelopeSignature::LENGTH]);
    let failing = MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::FIRST,
    );
    let untouched = MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::new(2).expect("a non-zero sequence"),
    );
    harness.deliveries.record(first, failing);
    harness.deliveries.record(second, untouched);

    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature: first,
        reason: DirectMessageFailure::Refused,
    });

    assert_eq!(
        harness.recorder.calls(),
        vec![Call::Failed(failing, DeliveryFailure::RetriesExhausted)]
    );
    // The second message is still outstanding, so its own acknowledgement can
    // still arrive and be correlated.
    assert_eq!(harness.deliveries.take(&second), Some(untouched));
}

#[test]
fn each_transport_failure_carries_its_own_delivery_reason_to_the_port() {
    let expected = [
        (
            DirectMessageFailure::PeerUnreachable,
            DeliveryFailure::PeerUnreachable,
        ),
        (
            DirectMessageFailure::SessionClosed,
            DeliveryFailure::SessionClosed,
        ),
        (
            DirectMessageFailure::NotAcknowledged,
            DeliveryFailure::RetriesExhausted,
        ),
        (
            DirectMessageFailure::Refused,
            DeliveryFailure::RetriesExhausted,
        ),
    ];

    for (index, (transport, delivery)) in expected.into_iter().enumerate() {
        let harness = harness();
        let signature = EnvelopeSignature::new([index as u8; EnvelopeSignature::LENGTH]);
        let message = MessageId::new(
            alice(),
            ConversationId::Direct(bob()),
            SequenceNumber::FIRST,
        );
        harness.deliveries.record(signature, message);

        harness.router.route(NetworkEvent::DirectMessageFailed {
            peer: bob(),
            signature,
            reason: transport,
        });

        assert_eq!(
            harness.recorder.calls(),
            vec![Call::Failed(message, delivery)],
            "{transport:?}"
        );
    }
}

#[test]
fn a_conversations_refusal_is_reported_as_it_stands_and_never_reinterpreted() {
    // A broadcast has no failed state and an already-terminal message keeps
    // what the user was shown. Both come back typed; the root states them.
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    harness.deliveries.record(
        signature,
        MessageId::new(alice(), ConversationId::Broadcast, SequenceNumber::FIRST),
    );
    *harness
        .recorder
        .delivery_failure_refusal
        .lock()
        .expect("no panic") = Some(MessagingCommandError::Conversation(
        messaging::domain::ConversationError::UnknownMessage,
    ));

    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature,
        reason: DirectMessageFailure::Refused,
    });

    assert_eq!(harness.diagnostics.port_refusals(), 1);
    assert!(
        harness
            .notices
            .all()
            .iter()
            .any(|notice| notice.text.contains("no such message")),
        "the conversation's own words must reach the user"
    );
}

#[test]
fn a_failure_for_an_unknown_signature_marks_nothing_and_is_still_stated() {
    // Evicted, or already answered. Failing a guessed message would mark the
    // wrong one failed — and a late failure must not overturn a message
    // already shown as delivered.
    let harness = harness();

    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature: EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]),
        reason: DirectMessageFailure::SessionClosed,
    });

    assert!(harness.recorder.calls().is_empty());
    assert_eq!(harness.diagnostics.direct_delivery_failures(), 1);
    assert_eq!(harness.diagnostics.uncorrelated_reports(), 1);
    assert!(
        harness
            .notices
            .all()
            .iter()
            .any(|notice| notice.text.contains("not delivered")),
        "a failure the user cannot see is silent loss"
    );
}

#[test]
fn nothing_probed_yet_is_unknown_and_is_not_unreachable() {
    // Startup, before any probe has concluded. S3: `Unknown` and `Unreachable`
    // are different facts, and a root that started life at `Unreachable` would
    // put a false verdict on screen during every launch.
    let harness = harness();

    assert_eq!(harness.diagnostics.reachability(), Reachability::Unknown);
    assert_ne!(
        harness.diagnostics.reachability(),
        Reachability::Unreachable
    );
}

#[test]
fn a_reachability_verdict_is_held_and_calls_no_port_at_all() {
    // The one variant that maps onto no inbound port (canvas D5): a fact about
    // this process's position on the network, owned by no context.
    let harness = harness();
    let endpoint = endpoint("/ip4/203.0.113.7/tcp/4001");

    harness
        .router
        .route(NetworkEvent::ReachabilityChanged(Reachability::Reachable(
            endpoint.clone(),
        )));

    assert!(harness.recorder.calls().is_empty());
    assert_eq!(
        harness.diagnostics.reachability(),
        Reachability::Reachable(endpoint)
    );
}

#[test]
fn a_verdict_changes_no_behaviour_it_only_reports() {
    // D4/S5: the verdict reports and libp2p decides. Nothing here announces,
    // dials, or touches the address set — an `Unreachable` peer that started
    // re-announcing or stopped advertising an address would be this piece
    // changing connectivity on evidence it explicitly does not trust that far.
    let harness = harness();
    harness
        .router
        .route(NetworkEvent::ListeningOn(endpoint("/ip4/10.0.0.1/tcp/1")));

    harness
        .router
        .route(NetworkEvent::ReachabilityChanged(Reachability::Unreachable));

    assert!(harness.recorder.calls().is_empty(), "no port was called");
    assert_eq!(
        harness.endpoints.all(),
        vec![endpoint("/ip4/10.0.0.1/tcp/1")],
        "the announced address set is untouched"
    );
    assert!(harness.notices.all().is_empty());
    assert_eq!(harness.diagnostics.port_refusals(), 0);
}

#[test]
fn the_latest_verdict_is_the_one_held() {
    // The event fires only on a transition, so what last arrived is what is
    // true now — including the return to `Reachable` that P2-7 requires not be
    // a one-way latch.
    let harness = harness();
    let endpoint = endpoint("/ip4/203.0.113.7/tcp/4001");

    harness
        .router
        .route(NetworkEvent::ReachabilityChanged(Reachability::Reachable(
            endpoint.clone(),
        )));
    harness
        .router
        .route(NetworkEvent::ReachabilityChanged(Reachability::Unreachable));
    assert_eq!(
        harness.diagnostics.reachability(),
        Reachability::Unreachable
    );

    harness
        .router
        .route(NetworkEvent::ReachabilityChanged(Reachability::Reachable(
            endpoint.clone(),
        )));

    assert_eq!(
        harness.diagnostics.reachability(),
        Reachability::Reachable(endpoint)
    );
}

#[test]
fn a_delivered_message_cannot_then_be_failed_by_a_late_report() {
    // The index consumes a signature, so the second report finds nothing to
    // name and never reaches the conversation at all.
    let harness = harness();
    let signature = EnvelopeSignature::new([9; EnvelopeSignature::LENGTH]);
    let message = MessageId::new(
        alice(),
        ConversationId::Direct(bob()),
        SequenceNumber::FIRST,
    );
    harness.deliveries.record(signature, message);

    harness.router.route(NetworkEvent::DirectMessageDelivered {
        peer: bob(),
        signature,
    });
    harness.router.route(NetworkEvent::DirectMessageFailed {
        peer: bob(),
        signature,
        reason: DirectMessageFailure::NotAcknowledged,
    });

    assert_eq!(harness.recorder.calls(), vec![Call::Delivered(message)]);
}
