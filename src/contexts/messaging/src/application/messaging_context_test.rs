use std::sync::Arc;

use shared_types::PeerDisconnected;

use crate::application::test_context::{TestContext, TestContextBuilder, sequence};
use crate::domain::{ConversationId, DeliveryFailure, DeliveryState};
use crate::ports::port_fakes::{InMemorySequenceCounter, RecordingTransport};
use crate::ports::{
    InboundEnvelopePort, MessageTransportPort, MessagingQueryPort, PeerLifecyclePort,
    SequenceCounterPort,
};
use crate::test_peers;

/// One peer's context, with the transport it sent through so another peer's
/// context can be handed the very envelopes it emitted.
struct Peer {
    context: TestContext,
    transport: Arc<RecordingTransport>,
}

fn peer(local: shared_types::PeerId, counter: Arc<InMemorySequenceCounter>) -> Peer {
    let transport = Arc::new(RecordingTransport::default());
    let context = TestContextBuilder::for_local_peer(local)
        .with_counter(counter as Arc<dyn SequenceCounterPort + Send + Sync>)
        .with_transport(Arc::clone(&transport) as Arc<dyn MessageTransportPort + Send + Sync>)
        .build();

    Peer { context, transport }
}

#[test]
fn the_command_and_query_sides_see_one_registry() {
    // CQRS separates the paths, not the state. A message accepted through the
    // inbound port has to be visible through the query port immediately, or a
    // pane would render a conversation the network already changed.
    let alice = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );

    let outcome = alice
        .context
        .send_direct(test_peers::bob(), "one registry")
        .expect("sent");

    assert_eq!(
        alice
            .context
            .context
            .queries()
            .delivery_state(outcome.sent.id),
        Some(DeliveryState::Pending)
    );
    assert_eq!(
        alice
            .context
            .visible_text(ConversationId::Direct(test_peers::bob())),
        vec!["one registry"]
    );
}

#[test]
fn every_inbound_port_shares_the_same_conversations() {
    let alice = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );
    let outcome = alice
        .context
        .send_direct(test_peers::bob(), "in flight")
        .expect("sent");

    // A disconnect arriving on the lifecycle port must reach the conversation
    // the send port wrote to.
    alice
        .context
        .context
        .lifecycle()
        .peer_disconnected(PeerDisconnected {
            peer: test_peers::bob(),
        })
        .expect("handled");

    assert_eq!(
        alice
            .context
            .context
            .queries()
            .delivery_state(outcome.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
    );
}

#[test]
fn a_restarted_peer_is_still_heard_by_a_peer_that_stayed_online() {
    // AC16, end to end, and the failure D12 was written for: with in-memory
    // history a restarted peer used to resume at sequence 1, and every message
    // it sent was — correctly, by the receiver's own rules — classified a
    // duplicate. It went permanently mute while appearing, to itself, to work.
    //
    // The counter outlives the process because its domain of validity is the
    // identity, not the process. Here one `InMemorySequenceCounter` plays the
    // part of the store that survives; the contexts around it do not.
    let alices_counter = Arc::new(InMemorySequenceCounter::default());
    let bob = peer(
        test_peers::bob(),
        Arc::new(InMemorySequenceCounter::default()),
    );

    // --- alice's first process ---
    let first_run = peer(test_peers::alice(), Arc::clone(&alices_counter));
    first_run
        .context
        .publish_broadcast("before the restart")
        .expect("published");
    first_run
        .context
        .publish_broadcast("still before")
        .expect("published");

    for envelope in first_run.transport.published() {
        bob.context.accept(envelope).expect("bob hears alice");
    }
    assert_eq!(
        bob.context.visible_text(ConversationId::Broadcast),
        vec!["before the restart", "still before"]
    );
    drop(first_run);

    // --- alice restarts: new context, new conversations, same counter ---
    let second_run = peer(test_peers::alice(), Arc::clone(&alices_counter));
    let outcome = second_run
        .context
        .publish_broadcast("after the restart")
        .expect("published");

    assert_eq!(
        outcome.sent.id.sequence(),
        sequence(3),
        "the run continues rather than restarting at 1"
    );

    let verdict = bob
        .context
        .accept(second_run.transport.published()[0].clone())
        .expect("bob judges it");

    assert!(
        verdict.is_applied(),
        "a restarted peer is still heard: {verdict:?}"
    );
    assert_eq!(
        bob.context.visible_text(ConversationId::Broadcast),
        vec!["before the restart", "still before", "after the restart"]
    );
}

#[test]
fn without_the_surviving_counter_the_restarted_peer_goes_mute() {
    // The counterfactual that makes the test above mean something. A fresh
    // counter is what a peer looks like when the keypair is gone too — and then
    // starting at 1 is correct, because the identity is new. Sharing the
    // *identity* while resetting the counter is the broken combination, and
    // this is what it costs.
    let bob = peer(
        test_peers::bob(),
        Arc::new(InMemorySequenceCounter::default()),
    );

    let first_run = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );
    first_run
        .context
        .publish_broadcast("before")
        .expect("published");
    bob.context
        .accept(first_run.transport.published()[0].clone())
        .expect("heard");

    let second_run = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );
    second_run
        .context
        .publish_broadcast("after")
        .expect("published");
    let verdict = bob
        .context
        .accept(second_run.transport.published()[0].clone())
        .expect("judged");

    assert!(verdict.is_duplicate(), "this is the failure D12 prevents");
    assert_eq!(
        bob.context.visible_text(ConversationId::Broadcast),
        vec!["before"]
    );
}

#[test]
fn two_peers_hold_a_direct_conversation_naming_each_other() {
    // Each side's `Direct` conversation is identified by its counterpart, so
    // alice's `Direct(bob)` and bob's `Direct(alice)` are the same exchange
    // seen from two ends.
    let alice = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );
    let bob = peer(
        test_peers::bob(),
        Arc::new(InMemorySequenceCounter::default()),
    );

    alice
        .context
        .send_direct(test_peers::bob(), "hello bob")
        .expect("sent");
    let (recipient, envelope) = alice.transport.sent_direct()[0].clone();
    assert_eq!(recipient, test_peers::bob());
    bob.context.accept(envelope).expect("heard");

    bob.context
        .send_direct(test_peers::alice(), "hello alice")
        .expect("sent");
    let (_, reply) = bob.transport.sent_direct()[0].clone();
    alice.context.accept(reply).expect("heard");

    // Both ends hold both messages. Their *order* is by author in `PeerId`
    // order, not interleaved: nothing orders across authors because nothing
    // could — there is no global clock, no consensus, and a claimed send time
    // is the claimer's to invent. What the contract does promise is that the
    // two ends agree, because both derive the grouping from the same
    // deterministic key (AC13).
    let alices_view = alice
        .context
        .visible_text(ConversationId::Direct(test_peers::bob()));
    let bobs_view = bob
        .context
        .visible_text(ConversationId::Direct(test_peers::alice()));

    assert_eq!(alices_view, bobs_view);
    assert_eq!(alices_view.len(), 2);
    assert!(alices_view.contains(&"hello bob".to_owned()));
    assert!(alices_view.contains(&"hello alice".to_owned()));
}

#[test]
fn the_context_is_inert_until_it_is_called() {
    // No task, no socket, no timer, and above all no clock reading: the gap
    // sweep is the composition root's obligation, which is what keeps every
    // test in this crate free of real time (AC13).
    let alice = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );

    assert_eq!(alice.context.events(), Vec::new());
    assert_eq!(
        alice
            .context
            .context
            .queries()
            .conversations()
            .expect("log"),
        Vec::new()
    );
    assert_eq!(
        alice
            .context
            .context
            .inbound()
            .close_aged_gaps()
            .expect("sweep"),
        Vec::new()
    );
}

#[test]
fn the_context_splits_into_parts_that_still_share_its_registry() {
    let alice = peer(
        test_peers::alice(),
        Arc::new(InMemorySequenceCounter::default()),
    );
    let outcome = alice
        .context
        .send_direct(test_peers::bob(), "before the split")
        .expect("sent");

    let (_send, _inbound, lifecycle, queries) = alice.context.context.into_parts();
    lifecycle
        .peer_disconnected(PeerDisconnected {
            peer: test_peers::bob(),
        })
        .expect("handled");

    assert_eq!(
        queries.delivery_state(outcome.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::SessionClosed))
    );
}
