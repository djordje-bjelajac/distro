use std::sync::Arc;

use membership::domain::Endpoint;
use membership::ports::{PeerDiscoveryError, PeerTransportError};
use messaging::ports::MessageTransportError;
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::clock::VirtualClock;
use crate::crypto::SimKeypair;
use crate::fabric::{DialFault, DropCause, SimFabric, SimFrame};

const SEED: u64 = 0xFAB;

struct Wired {
    fabric: Arc<SimFabric>,
    clock: Arc<VirtualClock>,
    peers: Vec<PeerId>,
}

fn wire(labels: &[&str]) -> Wired {
    let clock = Arc::new(VirtualClock::new());
    let fabric = Arc::new(SimFabric::new(Arc::clone(&clock), SEED));

    let peers = labels
        .iter()
        .map(|label| {
            let peer = SimKeypair::derived(SEED, label).peer();
            fabric.register(peer, label);
            peer
        })
        .collect();

    Wired {
        fabric,
        clock,
        peers,
    }
}

fn envelope(author: PeerId, kind: PayloadKind) -> Envelope {
    Envelope {
        version: ProtocolVersion::CURRENT,
        kind,
        author,
        payload: Vec::new(),
        signature: EnvelopeSignature::new([7; EnvelopeSignature::LENGTH]),
    }
}

fn connect(wired: &Wired, from: PeerId, to: PeerId) -> Endpoint {
    wired
        .fabric
        .dial(from, to, &wired.fabric.endpoints_of(to))
        .expect("a fresh link always answers")
}

fn drain(wired: &Wired) {
    while wired.fabric.take_due(wired.clock.now_millis()).is_some() {}
}

#[test]
fn a_registered_peer_listens_at_its_own_address() {
    let wired = wire(&["alice"]);

    let endpoints = wired
        .fabric
        .listen(wired.peers[0])
        .expect("a registered peer can listen");

    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].address(), "sim://alice");
}

#[test]
fn a_peer_that_cannot_listen_reports_it_rather_than_pretending() {
    let wired = wire(&["alice"]);
    wired.fabric.set_can_listen(wired.peers[0], false);

    assert_eq!(
        wired.fabric.listen(wired.peers[0]),
        Err(PeerTransportError::ListenFailed)
    );
}

#[test]
fn a_dial_puts_the_handshake_in_flight_and_holds_nothing_back() {
    // Two frames, both queued: the fabric never hands anything over on its own.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);

    connect(&wired, alice, bob);

    assert_eq!(wired.fabric.pending_frames(), 2);
    assert!(wired.fabric.has_link(alice, bob));
    assert!(
        wired.fabric.has_link(bob, alice),
        "the link is bidirectional"
    );
}

#[test]
fn dialling_an_address_that_belongs_to_nobody_finds_no_endpoint() {
    let wired = wire(&["alice", "bob"]);
    let stranger = Endpoint::direct("sim://nowhere").expect("a valid address");

    assert_eq!(
        wired
            .fabric
            .dial(wired.peers[0], wired.peers[1], &[stranger]),
        Err(PeerTransportError::NoReachableEndpoint)
    );
}

#[test]
fn an_unreachable_dial_fault_answers_with_nothing() {
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    wired
        .fabric
        .set_dial_fault(alice, bob, DialFault::Unreachable);

    assert_eq!(
        wired
            .fabric
            .dial(alice, bob, &wired.fabric.endpoints_of(bob)),
        Err(PeerTransportError::NoReachableEndpoint)
    );
}

#[test]
fn a_handshake_fault_is_reported_apart_from_silence() {
    // The port keeps the two apart because they call for different words to a
    // user: nothing answered, versus something answered and refused us.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    wired
        .fabric
        .set_dial_fault(alice, bob, DialFault::HandshakeFailure);

    assert_eq!(
        wired
            .fabric
            .dial(alice, bob, &wired.fabric.endpoints_of(bob)),
        Err(PeerTransportError::HandshakeFailed)
    );
}

#[test]
fn a_severed_link_can_be_dialled_around_through_a_relaying_peer() {
    // AC12 at the fabric level: the direct path is gone, the network is not.
    let wired = wire(&["alice", "bob", "carol"]);
    let (alice, bob, carol) = (wired.peers[0], wired.peers[1], wired.peers[2]);

    wired.fabric.sever_link(alice, bob);
    wired.fabric.advertise_relay(bob, carol);

    let answered = wired
        .fabric
        .dial(alice, bob, &wired.fabric.endpoints_of(bob))
        .expect("the relayed endpoint answers");

    assert!(
        answered.is_relayed(),
        "a third peer is carrying the traffic, and the endpoint says so"
    );
}

#[test]
fn a_partition_cannot_be_relayed_around() {
    // The difference between a severed link and a split: a relay on the far
    // side is unreachable too.
    let wired = wire(&["alice", "bob", "carol"]);
    let (alice, bob, carol) = (wired.peers[0], wired.peers[1], wired.peers[2]);

    wired.fabric.advertise_relay(bob, carol);
    wired.fabric.set_partition_group(bob, 1);

    assert_eq!(
        wired
            .fabric
            .dial(alice, bob, &wired.fabric.endpoints_of(bob)),
        Err(PeerTransportError::NoReachableEndpoint)
    );
}

#[test]
fn closing_a_link_that_does_not_exist_says_so() {
    let wired = wire(&["alice", "bob"]);

    assert_eq!(
        wired.fabric.close(wired.peers[0], wired.peers[1]),
        Err(PeerTransportError::NoSuchSession)
    );
}

#[test]
fn discovery_reaches_only_the_same_lan_segment() {
    // AC2 and AC3 are two topologies rather than two mocks.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);

    wired
        .fabric
        .announce(alice, &[])
        .expect("alice can announce");
    wired.fabric.announce(bob, &[]).expect("bob can announce");

    assert_eq!(
        wired
            .fabric
            .observe(alice)
            .expect("discovery is running")
            .len(),
        1
    );

    wired.fabric.set_lan_segment(bob, 9);
    wired.fabric.mdns_tick();

    assert!(
        wired
            .fabric
            .observe(alice)
            .expect("discovery is running")
            .is_empty(),
        "a peer on another segment is not on this LAN"
    );
}

#[test]
fn a_peer_is_observed_once_per_announcement() {
    // The port's contract is "since the last call"; re-announcing is what makes
    // a peer visible again, which is how a simulated mDNS tick behaves.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    wired.fabric.announce(bob, &[]).expect("bob can announce");

    assert_eq!(wired.fabric.observe(alice).expect("running").len(), 1);
    assert_eq!(wired.fabric.observe(alice).expect("running").len(), 0);

    wired.fabric.mdns_tick();
    assert_eq!(wired.fabric.observe(alice).expect("running").len(), 1);
}

#[test]
fn a_quiet_lan_is_success_rather_than_failure() {
    // An empty result is the ordinary state of a first launch; `Isolated` is a
    // normal status, not an error.
    let wired = wire(&["alice"]);

    assert_eq!(wired.fabric.observe(wired.peers[0]), Ok(Vec::new()));
}

#[test]
fn a_refused_announcement_is_reported() {
    let wired = wire(&["alice"]);
    wired.fabric.set_announce_refused(wired.peers[0], true);

    assert_eq!(
        wired.fabric.announce(wired.peers[0], &[]),
        Err(PeerDiscoveryError::AnnouncementRejected)
    );
}

#[test]
fn a_direct_message_needs_a_session() {
    // D4: a 1:1 message travels over the authenticated session, so a send with
    // no session is a stated failure rather than a message that vanishes.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);

    assert_eq!(
        wired
            .fabric
            .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage)),
        Err(MessageTransportError::SessionClosed)
    );
}

#[test]
fn a_direct_message_to_a_stopped_peer_is_unreachable() {
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    wired.fabric.set_online(bob, false);

    assert_eq!(
        wired
            .fabric
            .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage)),
        Err(MessageTransportError::PeerUnreachable)
    );
}

#[test]
fn a_severed_link_with_no_relay_available_says_exactly_that() {
    // S7's known limit, stated rather than hidden.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    wired.fabric.sever_link(alice, bob);

    assert_eq!(
        wired
            .fabric
            .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage)),
        Err(MessageTransportError::NoRelayAvailable)
    );
}

#[test]
fn a_broadcast_reaches_every_peer_the_sender_can_reach() {
    let wired = wire(&["alice", "bob", "carol"]);
    let alice = wired.peers[0];

    wired
        .fabric
        .publish_broadcast(alice, &envelope(alice, PayloadKind::BroadcastMessage))
        .expect("gossip accepts it");

    assert_eq!(wired.fabric.pending_frames(), 2);
}

#[test]
fn a_broadcast_hops_through_an_intermediate_peer() {
    // Gossip, not fan-out from the sender: alice cannot reach carol directly,
    // and carol still hears her.
    let wired = wire(&["alice", "bob", "carol"]);
    let (alice, carol) = (wired.peers[0], wired.peers[2]);
    wired.fabric.sever_link(alice, carol);

    wired
        .fabric
        .publish_broadcast(alice, &envelope(alice, PayloadKind::BroadcastMessage))
        .expect("gossip accepts it");

    assert_eq!(wired.fabric.pending_frames(), 2);
}

#[test]
fn a_broadcast_stops_at_a_partition() {
    let wired = wire(&["alice", "bob", "carol"]);
    let (alice, carol) = (wired.peers[0], wired.peers[2]);
    wired.fabric.set_partition_group(carol, 1);

    wired
        .fabric
        .publish_broadcast(alice, &envelope(alice, PayloadKind::BroadcastMessage))
        .expect("gossip accepts it");

    assert_eq!(wired.fabric.pending_frames(), 1);
}

#[test]
fn a_broadcast_that_reaches_nobody_is_still_a_success() {
    let wired = wire(&["alice"]);
    let alice = wired.peers[0];

    assert_eq!(
        wired
            .fabric
            .publish_broadcast(alice, &envelope(alice, PayloadKind::BroadcastMessage)),
        Ok(())
    );
    assert_eq!(wired.fabric.pending_frames(), 0);
}

#[test]
fn nothing_becomes_due_before_its_delay_has_elapsed() {
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    drain(&wired);
    wired.fabric.set_link_delay(alice, bob, 50);

    wired
        .fabric
        .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage))
        .expect("the session is up");

    let sent_at = wired.clock.now_millis();
    assert_eq!(wired.fabric.next_due_at(), Some(sent_at + 50));
    assert!(wired.fabric.take_due(sent_at).is_none());

    wired.clock.advance(50);
    assert!(wired.fabric.take_due(wired.clock.now_millis()).is_some());
}

#[test]
fn a_delay_script_decides_the_delivery_order() {
    // The general primitive behind AC8's reorder injection and AC10's gossip
    // scrambling: three messages, an order written down, no chance involved.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    drain(&wired);

    wired.fabric.script_delays([30, 10, 20]);
    for _ in 0..3 {
        wired
            .fabric
            .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage))
            .expect("the session is up");
    }

    wired.clock.advance(30);
    let order: Vec<u64> = std::iter::from_fn(|| wired.fabric.take_due(wired.clock.now_millis()))
        .map(|frame| frame.id)
        .collect();

    // Enqueued as frames 2, 3, 4; due at +30, +10, +20, so they come out 3, 4, 2.
    assert_eq!(order, vec![3, 4, 2]);
    assert_eq!(wired.fabric.scripted_delays_remaining(), 0);
}

#[test]
fn session_frames_do_not_consume_the_message_delay_script() {
    let wired = wire(&["alice", "bob"]);
    wired.fabric.script_delays([5, 5]);

    connect(&wired, wired.peers[0], wired.peers[1]);

    assert_eq!(wired.fabric.scripted_delays_remaining(), 2);
}

#[test]
fn duplication_makes_the_same_message_arrive_twice() {
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    drain(&wired);

    wired.fabric.duplicate_next(1);
    for _ in 0..2 {
        wired
            .fabric
            .send_direct(alice, bob, &envelope(alice, PayloadKind::DirectMessage))
            .expect("the session is up");
    }

    assert_eq!(
        wired.fabric.pending_frames(),
        3,
        "the budget duplicated only the first message"
    );
}

#[test]
fn corruption_flips_a_signature_bit_on_the_wire() {
    // AC6's in-band forgery: genuine when handed over, corrupt when it arrives.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);
    drain(&wired);

    let original = envelope(alice, PayloadKind::DirectMessage);
    wired.fabric.corrupt_next_signatures(1);
    wired
        .fabric
        .send_direct(alice, bob, &original)
        .expect("the session is up");

    let delivered = wired
        .fabric
        .take_due(wired.clock.now_millis())
        .expect("a frame is due");

    match delivered.frame {
        SimFrame::Message(arrived) => assert_ne!(arrived.signature, original.signature),
        other => panic!("expected a message frame, got {other:?}"),
    }
}

#[test]
fn a_partition_that_comes_down_mid_flight_drops_what_was_on_the_wire() {
    // Evaluated at delivery rather than at enqueue, because that is what a
    // partition does to traffic already travelling.
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);

    assert_eq!(wired.fabric.delivery_block(alice, bob), None);

    wired.fabric.set_partition_group(bob, 1);

    assert_eq!(
        wired.fabric.delivery_block(alice, bob),
        Some(DropCause::Partitioned)
    );
}

#[test]
fn stopping_a_peer_drops_its_links_from_both_sides() {
    let wired = wire(&["alice", "bob"]);
    let (alice, bob) = (wired.peers[0], wired.peers[1]);
    connect(&wired, alice, bob);

    wired.fabric.set_online(bob, false);

    assert!(!wired.fabric.has_link(alice, bob));
    assert!(!wired.fabric.has_link(bob, alice));
    assert_eq!(
        wired.fabric.delivery_block(alice, bob),
        Some(DropCause::DestinationOffline)
    );
}

#[test]
fn delivery_order_is_the_same_for_two_identically_seeded_fabrics() {
    let orders: Vec<Vec<u64>> = (0..2)
        .map(|_| {
            let wired = wire(&["alice", "bob", "carol"]);
            let alice = wired.peers[0];

            wired.fabric.script_delays([7, 3]);
            wired
                .fabric
                .publish_broadcast(alice, &envelope(alice, PayloadKind::BroadcastMessage))
                .expect("gossip accepts it");

            wired.clock.advance(10);
            std::iter::from_fn(|| wired.fabric.take_due(wired.clock.now_millis()))
                .map(|frame| frame.id)
                .collect()
        })
        .collect();

    assert_eq!(orders[0], orders[1]);
}
