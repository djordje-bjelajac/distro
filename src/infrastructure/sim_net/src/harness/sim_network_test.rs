use membership::domain::{DurationMillis, NetworkStatus};
use messaging::domain::events::MessagingEvent;
use messaging::domain::{ConversationId, DeliveryFailure, DeliveryState, MessageId};
use shared_types::PeerId;

use crate::clock::VirtualClock;
use crate::harness::SimNetwork;

const SEED: u64 = 0xC0FFEE;

fn pair() -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .build()
}

// ---------------------------------------------------------------------------
// Determinism self-tests (canvas OP-8: "harness self-tests proving determinism")
// ---------------------------------------------------------------------------

#[test]
fn the_clock_never_advances_on_its_own() {
    // Property 1 of the determinism contract. Nothing in this crate reads real
    // time, so a whole scenario's worth of work leaves the clock exactly where
    // the scenario put it. Only `advance` and `settle` move it, and neither is
    // called here.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    for peer in [alice, bob] {
        net.initialize(peer);
        net.peer(peer).join().expect("the publisher is healthy");
        net.pump();
    }

    net.named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    net.named("bob")
        .publish_broadcast("hi all")
        .expect("gossip");
    net.pump();
    net.tick();
    net.heartbeat_tick();
    net.pump();

    assert_eq!(net.now(), VirtualClock::EPOCH_MILLIS);
}

#[test]
fn the_fabric_delivers_nothing_without_an_explicit_pump() {
    // Property 2. A message handed to the transport is in flight and nowhere
    // else: no thread carries it, and no read model has seen it.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");

    assert!(net.pending_frames() > 0, "the message is in flight");
    assert!(
        net.peer(bob).direct_history(alice).is_empty(),
        "nothing arrived without a pump"
    );

    net.pump();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["hello"]
    );
}

#[test]
fn advancing_the_clock_alone_delivers_nothing() {
    // The two controls are genuinely separate: time passing makes a frame
    // *due*, and only a pump hands it over.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();
    net.set_default_delay(100);

    net.named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    net.advance(1_000);

    assert_eq!(net.pending_frames(), 1);
    assert!(net.peer(bob).direct_history(alice).is_empty());
}

#[test]
fn the_same_seed_and_script_produce_byte_identical_traces() {
    // Property 3, and the one the canvas names outright. The comparison is on
    // rendered text, so it covers the interleaving of every peer's events with
    // every frame the fabric moved, at every virtual instant.
    let first = run_reference_scenario();
    let second = run_reference_scenario();

    assert_eq!(
        first, second,
        "two runs of one script diverged; the simulation is not deterministic"
    );
    // A guard against the reference scenario quietly becoming trivial: an empty
    // trace compares equal to another empty trace and proves nothing.
    assert!(
        first.lines().count() > 25,
        "the reference scenario stopped exercising the system: {first}"
    );
}

#[test]
fn a_different_seed_still_produces_a_stable_trace() {
    // Determinism is a property of a run, not of one lucky seed.
    let build = || {
        SimNetwork::seeded(1)
            .with_peers(["alice", "bob", "carol"])
            .build()
    };

    assert_eq!(exercise(build()), exercise(build()));
}

/// A scenario touching every moving part: discovery, sessions, both message
/// paths, reordering, duplication, a sweep, and a departure.
fn run_reference_scenario() -> String {
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol"])
        .build();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot_all();

    // Scrambled gossip and a duplicate, then the network settles.
    net.script_delays([40, 10]);
    net.duplicate_next(1);
    net.named("alice")
        .publish_broadcast("first")
        .expect("gossip accepts it");
    net.settle();

    net.peer(bob)
        .send_direct(alice, "and a direct")
        .expect("the session is up");
    net.settle();

    // A departure nobody announced, noticed by presence expiry.
    net.stop(carol);
    net.run_for(70_000);

    net.render_trace()
}

fn exercise(net: SimNetwork) -> String {
    net.boot_all();
    net.named("alice")
        .publish_broadcast("anything")
        .expect("gossip accepts it");
    net.settle();
    net.render_trace()
}

// ---------------------------------------------------------------------------
// Assembly: the wiring a composition root would do
// ---------------------------------------------------------------------------

#[test]
fn a_fresh_peer_has_assumed_nothing_until_it_is_started() {
    // AC1's "before": a process that has been launched and no more, so a
    // scenario can assert on first launch rather than be handed its result.
    let net = pair();

    assert!(net.named("alice").local_identity().is_none());
    assert_eq!(net.named("alice").network_status(), NetworkStatus::Isolated);
}

#[test]
fn a_peer_id_is_stable_across_a_restart() {
    // AC9: the keypair persists locally and is created without interaction.
    let mut net = pair();
    let alice = net.peer_id("alice");
    net.boot(alice);

    let before = net.peer(alice).local_identity().expect("assumed").peer;
    net.restart(alice);
    let after = net.peer(alice).local_identity().expect("assumed").peer;

    assert_eq!(before, after);
    assert_eq!(after, alice);
}

#[test]
fn two_peers_on_one_lan_discover_each_other_and_connect_unconfigured() {
    // AC2 at the harness level: no configuration, no ticket, no addresses typed
    // anywhere.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot_all();

    assert!(net.named("alice").is_connected_to(bob));
    assert!(net.named("bob").is_connected_to(alice));
    assert_eq!(
        net.named("alice").network_status(),
        NetworkStatus::from_connected_peers(1)
    );
}

#[test]
fn a_peer_alone_on_its_lan_ends_isolated_with_a_diagnostic() {
    // AC3: failure produces a visible diagnostic, never a hang. Every rung is
    // named, and `Isolated` is a normal state rather than an error.
    let net = pair();
    let alice = net.peer_id("alice");
    net.isolate_from_lan(alice);
    net.initialize(alice);

    let outcome = net.peer(alice).join().expect("the publisher is healthy");

    assert!(!outcome.succeeded());
    assert_eq!(outcome.status, NetworkStatus::Isolated);
    assert_eq!(outcome.diagnostic.rungs_tried().len(), 3);
}

#[test]
fn an_isolated_peer_joins_with_a_ticket_from_a_peer_it_cannot_see() {
    // D1's third rung: the one human step, and the only one that crosses a LAN.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot(alice);

    net.isolate_from_lan(bob);
    net.initialize(bob);

    let ticket = net.join_ticket_from(alice);
    let outcome = net
        .peer(bob)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");
    net.settle();

    assert!(outcome.succeeded());
    assert!(net.named("bob").is_connected_to(alice));
}

#[test]
fn an_expired_ticket_is_refused_before_it_reaches_the_network() {
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot(alice);
    net.isolate_from_lan(bob);
    net.initialize(bob);

    let ticket = net.join_ticket_expiring_after(alice, DurationMillis::ZERO);
    let outcome = net
        .peer(bob)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");

    assert!(!outcome.succeeded());
    assert!(matches!(
        outcome
            .diagnostic
            .failure_of(membership::ports::BootstrapRung::JoinTicket),
        Some(membership::ports::RungFailure::Ticket(_))
    ));
}

#[test]
fn both_signer_ports_are_wired_to_the_one_key_so_messages_verify() {
    // Canvas §4's cross-context wiring, end to end: alice signs with the key
    // her identity context holds, and bob's verifier accepts it against the
    // `PeerId` alone.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.named("alice")
        .publish_broadcast("signed by a real key")
        .expect("gossip accepts it");
    net.settle();

    assert_eq!(net.peer(bob).broadcast_history().len(), 1);
    assert_eq!(net.peer(bob).broadcast_history()[0].author(), alice);
}

#[test]
fn a_forged_signature_never_reaches_a_read_model() {
    // AC6 and invariant 10, staged in band: the envelope is genuine when it is
    // handed over and corrupt when it arrives.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();
    net.clear_trace();

    net.corrupt_next_signatures(1);
    net.named("alice")
        .publish_broadcast("tampered in flight")
        .expect("gossip accepts it");
    net.settle();

    assert!(net.peer(bob).broadcast_history().is_empty());
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageRejected(_))),
        "the refusal was not counted in local diagnostics"
    );
    assert_ne!(alice, bob);
}

#[test]
fn blocking_a_peer_through_identity_stops_its_messages_in_messaging() {
    // Invariant 11's cross-context wiring: one block list, two contexts, no
    // import between them and nothing for a scenario to wire.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.named("bob").block(alice).expect("alice is not blocked");
    net.named("alice")
        .publish_broadcast("unwelcome")
        .expect("gossip accepts it");
    net.settle();

    assert!(net.peer(bob).broadcast_history().is_empty());
    assert_eq!(
        net.named("bob").blocked_peers().expect("healthy store"),
        vec![alice]
    );
}

// ---------------------------------------------------------------------------
// Delivery, ordering, and the restart the harness exists to model
// ---------------------------------------------------------------------------

#[test]
fn a_direct_message_moves_from_pending_to_delivered() {
    // AC11: silent loss is not a state, so the acknowledgement is part of the
    // simulated transport rather than something a scenario fakes.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let outcome = net
        .named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    assert_eq!(outcome.delivery, DeliveryState::Pending);

    net.settle();

    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Delivered)
    );
}

#[test]
fn acknowledgement_can_be_switched_off_to_hold_a_message_pending() {
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .acknowledging_directs(false)
        .build();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let outcome = net
        .named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    net.settle();

    assert_eq!(
        net.peer(alice).delivery_state(outcome.sent.id),
        Some(DeliveryState::Pending)
    );
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["hello"],
        "the recipient still has it; only the acknowledgement was suppressed"
    );
}

#[test]
fn a_reported_delivery_failure_moves_the_one_message_it_names_off_pending() {
    // AC11's other ending, at the seam a scenario drives it through. No frame
    // carries this report — a transport answers `send_direct` once it has
    // queued the request and refuses afterwards — so the harness exposes it as
    // a call, exactly as the composition root exposes `DirectMessageFailed`.
    // Acknowledgement is off so both messages are genuinely still pending when
    // it arrives.
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .acknowledging_directs(false)
        .build();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let refused = net
        .named("alice")
        .send_direct(bob, "refused later")
        .expect("the session is up");
    let held = net
        .named("alice")
        .send_direct(bob, "still waiting")
        .expect("the session is up");
    net.settle();

    let changed = net
        .peer(alice)
        .message_delivery_failed(refused.sent.id, DeliveryFailure::RetriesExhausted)
        .expect("a pending direct may fail");

    assert_eq!(changed.from, DeliveryState::Pending);
    assert_eq!(
        changed.to,
        DeliveryState::Failed(DeliveryFailure::RetriesExhausted)
    );
    assert_eq!(
        net.peer(alice).delivery_state(refused.sent.id),
        Some(DeliveryState::Failed(DeliveryFailure::RetriesExhausted))
    );
    assert_eq!(
        net.peer(alice).delivery_state(held.sent.id),
        Some(DeliveryState::Pending),
        "one refusal is news about one message"
    );
    assert!(
        net.trace()
            .messaging_events_of(alice)
            .iter()
            .any(|event| matches!(
                event,
                MessagingEvent::MessageDeliveryStateChanged(announced)
                    if announced.to == DeliveryState::Failed(DeliveryFailure::RetriesExhausted)
            )),
        "the failure was not announced"
    );
}

#[test]
fn a_delivery_failure_naming_no_message_is_refused_rather_than_panicking() {
    // A report the conversation cannot place means the transport and the
    // conversation disagree about what was sent. The passthrough hands that
    // back as a typed error, so a scenario staging one sees a `Result` rather
    // than a wedged run — and the message that does exist is left alone.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    let real = net
        .named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    net.settle();

    let never_sent = MessageId::new(
        alice,
        ConversationId::Direct(bob),
        messaging::domain::SequenceNumber::MAX,
    );

    assert!(
        net.peer(alice)
            .message_delivery_failed(never_sent, DeliveryFailure::PeerUnreachable)
            .is_err(),
        "a refusal naming nothing was accepted"
    );
    assert_eq!(
        net.peer(alice).delivery_state(real.sent.id),
        Some(DeliveryState::Delivered),
        "a stray report disturbed a message it did not name"
    );
}

#[test]
fn a_redelivered_message_changes_nothing_user_visible() {
    // AC7: exactly-once application over at-least-once delivery.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.duplicate_next(1);
    net.named("alice")
        .publish_broadcast("said once")
        .expect("gossip accepts it");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["said once"]
    );
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageDuplicateIgnored(_))),
        "the duplicate was not counted"
    );
    assert_ne!(alice, bob);
}

#[test]
fn a_scripted_reorder_still_displays_in_the_authors_send_order() {
    // AC8: arrival order is the scenario's to decide, and display order is not.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.script_delays([60, 20]);
    net.named("alice")
        .publish_broadcast("first")
        .expect("gossip");
    net.named("alice")
        .publish_broadcast("second")
        .expect("gossip");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["first", "second"]
    );
    assert_ne!(alice, bob);
}

#[test]
fn a_stopped_peer_is_noticed_within_the_liveness_window() {
    // AC5: stopping any instance leaves the others functional, and the
    // departure is observed rather than guessed at.
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
        .build();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    assert_eq!(net.named("alice").online_peers(), vec![bob]);

    net.stop(bob);
    net.run_for(70_000);

    assert!(net.named("alice").online_peers().is_empty());
    assert_eq!(net.named("alice").network_status().connected_peers(), 1);
    assert_ne!(alice, bob);
}

#[test]
fn heartbeats_keep_a_quiet_peer_online() {
    let net = pair();
    let bob = net.peer_id("bob");
    net.boot_all();

    for _ in 0..10 {
        net.run_for(5_000);
    }

    assert_eq!(net.named("alice").online_peers(), vec![bob]);
}

#[test]
fn a_restarted_peer_keeps_its_sequence_and_loses_its_history() {
    // D12 and AC16 together, which is the whole reason the harness splits
    // durable state from process state. Before D12 the third message below was
    // classified a duplicate by every listener and the peer went mute.
    let mut net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    for text in ["one", "two"] {
        net.named("alice").publish_broadcast(text).expect("gossip");
    }
    net.settle();

    net.restart(alice);

    assert!(
        net.peer(alice).broadcast_history().is_empty(),
        "conversation history must die with the process (D7)"
    );
    assert_eq!(
        net.peer(alice)
            .durable()
            .counter()
            .mark(ConversationId::Broadcast)
            .map(messaging::domain::SequenceNumber::as_u64),
        Some(2),
        "the outbound counter shares the keypair's lifetime (D12)"
    );

    net.named("alice")
        .publish_broadcast("three")
        .expect("gossip accepts it");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["one", "two", "three"],
        "the restarted peer was still heard (AC16)"
    );
}

#[test]
fn a_restart_keeps_the_warm_peer_cache() {
    // D1's first rung is what makes a join ticket a one-time cost.
    let mut net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.peer(alice).leave().expect("the publisher is healthy");
    net.settle();
    assert!(net.peer(alice).durable().cache().holds(bob));

    net.restart(alice);

    assert!(
        net.peer(alice).durable().cache().holds(bob),
        "the cache must outlive the process"
    );
}

#[test]
fn a_severed_link_is_routed_around_by_a_third_peer() {
    // AC12's logical layer: two peers that cannot connect directly still
    // communicate, and the relay never reads what it carries.
    let net = SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol"])
        .build();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot(alice);
    net.boot(carol);

    // Neither can dial the other directly; carol offers circuit service to both
    // (AC4 — every instance does).
    net.sever_link(alice, bob);
    net.advertise_relay(alice, carol);
    net.advertise_relay(bob, carol);

    // Driven explicitly rather than through the bootstrap ladder, so the test
    // pins the relayed dial rather than whichever candidate the ladder reached
    // first.
    net.initialize(bob);
    net.peer(bob)
        .peer_observed(membership::ports::DiscoveredPeer {
            peer: alice,
            endpoints: net.fabric().endpoints_of(alice),
        })
        .expect("the roster accepts a discovery");
    net.peer(bob)
        .connect_to(alice)
        .expect("the relayed endpoint answers");
    net.settle();

    net.peer(bob)
        .send_direct(alice, "through carol")
        .expect("a relayed path exists");
    net.settle();

    assert_eq!(
        net.peer(alice).transcript(ConversationId::Direct(bob)),
        ["through carol"]
    );
    assert!(
        net.peer(carol).direct_history(bob).is_empty(),
        "the relay routes the frame without its messaging context ever seeing it"
    );
}

#[test]
fn a_partition_stops_traffic_and_healing_restores_it() {
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.boot_all();

    net.partition_off(&[bob]);
    net.named("alice")
        .publish_broadcast("into the void")
        .expect("gossip accepts it");
    net.settle();

    assert!(net.peer(bob).broadcast_history().is_empty());

    net.heal_partitions();
    net.named("alice")
        .publish_broadcast("after the heal")
        .expect("gossip accepts it");
    net.settle();

    // The message that survived carries sequence 2, and bob never saw 1 — so it
    // waits behind the gap until the tolerance window elapses and a sweep
    // abandons the range (rule R, AC10's affirmative half, AC15). No history is
    // replayed: what was lost to the split stays lost, and is reported.
    assert!(net.peer(bob).broadcast_history().is_empty());
    net.run_for(3_000);

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["after the heal"],
        "the message lost to the split is not resurrected"
    );
    assert!(
        net.trace()
            .messaging_events_of(bob)
            .iter()
            .any(|event| matches!(event, MessagingEvent::MessageGapClosed(_))),
        "the abandoned range was not reported"
    );
    assert_ne!(alice, bob);
}

// ---------------------------------------------------------------------------
// Harness mechanics
// ---------------------------------------------------------------------------

#[test]
fn settling_leaves_the_network_quiet() {
    let net = pair();
    let bob = net.peer_id("bob");
    net.boot_all();
    net.set_default_delay(25);

    net.named("alice")
        .send_direct(bob, "hello")
        .expect("the session is up");
    net.settle();

    assert!(net.is_quiescent());
    assert_eq!(net.pending_frames(), 0);
}

#[test]
fn pumping_once_hands_over_exactly_one_frame() {
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    net.initialize(alice);
    net.initialize(bob);
    net.peer(alice).join().expect("the publisher is healthy");
    net.peer(bob).join().expect("the publisher is healthy");

    let before = net.pending_frames();
    assert!(before > 1);
    assert!(net.pump_once());
    assert_eq!(net.pending_frames(), before - 1);
}

#[test]
fn pumping_an_empty_network_reports_that_nothing_moved() {
    let net = pair();

    assert!(!net.pump_once());
    assert_eq!(net.pump(), 0);
    assert!(net.is_quiescent());
}

#[test]
fn the_seeded_stream_is_the_only_chance_in_a_run() {
    let first = SimNetwork::seeded(42).build();
    let second = SimNetwork::seeded(42).build();

    let left: Vec<u64> = (0..16).map(|_| first.random_below(1_000)).collect();
    let right: Vec<u64> = (0..16).map(|_| second.random_below(1_000)).collect();

    assert_eq!(left, right);
    assert_eq!(first.seed(), 42);
}

#[test]
fn peers_keep_the_order_a_scenario_added_them_in() {
    let net = SimNetwork::seeded(SEED)
        .with_peers(["zoe", "alice", "mallory"])
        .build();

    let labels: Vec<String> = net.peers().map(|peer| peer.label().to_owned()).collect();

    assert_eq!(labels, vec!["zoe", "alice", "mallory"]);
    assert_eq!(net.peer_ids().len(), 3);
}

#[test]
fn the_trace_names_peers_by_the_labels_a_scenario_chose() {
    let net = pair();
    net.boot_all();

    let rendered = net.render_trace();

    assert!(rendered.contains("alice"), "{rendered}");
    assert!(rendered.contains("bob"), "{rendered}");
    assert!(
        !rendered.contains("peer:"),
        "an unlabelled peer leaked into the trace: {rendered}"
    );
}

#[test]
#[should_panic(expected = "no peer named")]
fn addressing_a_peer_that_does_not_exist_fails_at_the_lookup() {
    let net = pair();
    let _: PeerId = net.peer_id("mallory");
}

#[test]
#[should_panic(expected = "already in this network")]
fn two_peers_cannot_share_one_name() {
    let mut net = pair();
    net.add_peer("alice");
}
