//! What happens when the network itself misbehaves: a path that cannot be
//! dialled, and a network that splits in two (canvas AC5, AC12, safeguard S7).

use std::sync::Arc;

use infra_sim_net::{SimNetwork, SimPeerTransport};
use membership::domain::events::MembershipEvent;
use membership::domain::{Endpoint, LivenessWindows, Reachability};
use membership::ports::{DiscoveredPeer, PeerTransportError, PeerTransportPort};
use messaging::domain::ConversationId;
use messaging::domain::events::{GapCloseCause, MessagingEvent};
use shared_types::PeerId;

/// One seed for the whole file, written down rather than picked implicitly
/// (AC13).
const SEED: u64 = 90_004;

fn network() -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob", "carol"])
        .build()
}

/// The gap-tolerance window every peer in `net` was assembled with (rule R).
fn gap_tolerance(net: &SimNetwork) -> u64 {
    net.settings().gap_tolerance.as_millis()
}

/// Connects every peer to every other, which `boot_all` does not: it connects
/// each peer to the first candidate that answers.
fn connect_every_pair(net: &SimNetwork) {
    let peers = net.peer_ids();

    for (index, one) in peers.iter().enumerate() {
        for other in &peers[index + 1..] {
            if net.peer(*one).is_connected_to(*other) {
                continue;
            }

            net.peer(*one)
                .peer_observed(DiscoveredPeer {
                    peer: *other,
                    endpoints: net.fabric().endpoints_of(*other),
                })
                .expect("the roster accepts a discovery");
            net.peer(*one)
                .connect_to(*other)
                .expect("an online peer on the same network answers");
            net.settle();
        }
    }
}

/// What `reader` displays of `author`'s side of a conversation, in the order it
/// would display it.
///
/// A 1:1 conversation holds both participants' messages, so a two-way exchange
/// is asserted one author at a time: what each side *received* is the claim,
/// and its own outbox is not evidence of it.
fn heard_from(
    net: &SimNetwork,
    reader: PeerId,
    conversation: ConversationId,
    author: PeerId,
) -> Vec<String> {
    net.peer(reader)
        .history(conversation)
        .iter()
        .filter(|message| message.author() == author)
        .map(|message| message.body().to_string())
        .collect()
}

fn expired_presence_for(net: &SimNetwork, observer: PeerId, subject: PeerId) -> bool {
    net.trace()
        .membership_events_of(observer)
        .iter()
        .any(|event| {
            matches!(
                event,
                MembershipEvent::PeerPresenceExpired(expired) if expired.peer == subject
            )
        })
}

// ---------------------------------------------------------------------------
// AC12 — a third peer carries what a direct path cannot
// ---------------------------------------------------------------------------

#[test]
fn only_a_relayed_endpoint_answers_when_the_direct_path_is_severed() {
    // AC12, at the transport boundary `membership` dials through: with the
    // direct path down, the endpoint that answers says a third *peer* is
    // carrying the traffic. Every instance offers that service (AC4) — no
    // operator runs a relay (S1).
    let net = network();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot(alice);
    net.boot(carol);
    net.sever_link(alice, bob);
    net.advertise_relay(alice, carol);

    let transport = SimPeerTransport::new(bob, Arc::clone(net.fabric()));
    let published = net.fabric().endpoints_of(alice);
    let (relayed, direct): (Vec<Endpoint>, Vec<Endpoint>) =
        published.iter().cloned().partition(Endpoint::is_relayed);

    assert!(!direct.is_empty(), "alice publishes a direct address");
    assert_eq!(
        relayed.len(),
        1,
        "carol's circuit is published exactly once"
    );

    // The direct address alone answers nothing — the path really is down.
    assert_eq!(
        transport.dial(alice, &direct),
        Err(PeerTransportError::NoReachableEndpoint)
    );

    // Offered every address, the one that answers is the relayed one.
    let answered = transport
        .dial(alice, &published)
        .expect("a peer relay bridges the severed path");

    assert_eq!(answered.reachability(), Reachability::Relayed);
    assert!(answered.is_relayed());
    assert_eq!(answered, relayed[0]);
}

#[test]
fn two_peers_that_cannot_dial_each_other_exchange_messages_through_a_third_peer() {
    // AC12 end to end: two peers with no direct path hold a 1:1 conversation
    // through a third peer, in both directions.
    let net = network();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot(alice);
    net.boot(carol);

    net.sever_link(alice, bob);
    net.advertise_relay(alice, carol);
    net.advertise_relay(bob, carol);

    // Driven explicitly rather than through the bootstrap ladder, so the
    // scenario pins the relayed dial rather than whichever candidate the ladder
    // happened to reach first.
    net.initialize(bob);
    net.peer(bob)
        .peer_observed(DiscoveredPeer {
            peer: alice,
            endpoints: net.fabric().endpoints_of(alice),
        })
        .expect("the roster accepts a discovery");
    net.peer(bob)
        .connect_to(alice)
        .expect("the relayed endpoint answers");
    net.settle();

    assert!(net.peer(bob).is_connected_to(alice));
    assert!(net.peer(alice).is_connected_to(bob));
    let known = net
        .peer(bob)
        .known_peers()
        .into_iter()
        .find(|view| view.peer == alice)
        .expect("bob knows the peer it dialled");
    assert!(
        known.endpoints.iter().any(Endpoint::is_relayed),
        "the roster does not hold the relayed address the dial used"
    );

    net.peer(bob)
        .send_direct(alice, "through carol")
        .expect("a relayed path exists");
    net.settle();
    net.peer(alice)
        .send_direct(bob, "and back again")
        .expect("a relayed path exists");
    net.settle();

    assert_eq!(
        heard_from(&net, alice, ConversationId::Direct(bob), bob),
        ["through carol"]
    );
    assert_eq!(
        heard_from(&net, bob, ConversationId::Direct(alice), alice),
        ["and back again"]
    );

    // AC12's second clause says relayed bytes are ciphertext to the relay. That
    // is a transport property, and this layer cannot state it: the simulated
    // fabric routes the frame to its destination without ever handing it to
    // carol's inbound port, and the real claim belongs to the Noise transport
    // in `infra-net-libp2p` (D4, OP-10). What *is* provable here — and asserted
    // rather than assumed — is that nothing of the conversation reaches any of
    // the relay's read models.
    assert!(net.peer(carol).direct_history(alice).is_empty());
    assert!(net.peer(carol).direct_history(bob).is_empty());
    assert!(net.peer(carol).broadcast_history().is_empty());
    assert!(
        net.trace()
            .messaging_events_of(carol)
            .iter()
            .all(|event| !matches!(event, MessagingEvent::MessageReceived(_))),
        "the relay's messaging context took delivery of what it carried"
    );
}

// ---------------------------------------------------------------------------
// AC5 — a split network, and what each side can still say
// ---------------------------------------------------------------------------

#[test]
fn a_partition_expires_presence_on_both_sides_and_healing_restores_traffic() {
    // AC5: peers observe a departure within the liveness window, and a
    // partition is a departure seen from both sides. Each peer's view is
    // authoritative only for itself (invariant 9), so both sides are checked.
    let net = network();
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot_all();
    connect_every_pair(&net);

    net.peer(alice)
        .publish_broadcast("before the split")
        .expect("gossip accepts it");
    net.settle();
    for listener in [bob, carol] {
        assert_eq!(
            net.peer(listener).transcript(ConversationId::Broadcast),
            ["before the split"]
        );
    }

    net.partition_off(&[carol]);

    net.peer(alice)
        .publish_broadcast("during the split")
        .expect("gossip accepts it");
    net.settle();
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Broadcast),
        ["before the split", "during the split"],
        "the intact side stopped working"
    );
    assert_eq!(
        net.peer(carol).transcript(ConversationId::Broadcast),
        ["before the split"],
        "a message crossed a partition"
    );

    let split_at = net.now();
    while net.now() - split_at < LivenessWindows::DEFAULT_OFFLINE.as_millis() {
        net.run_for(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());
    }

    // Both sides noticed, each about the other, and neither asserted anything
    // about a peer it could not see (invariant 7).
    assert!(!net.peer(alice).online_peers().contains(&carol));
    assert!(!net.peer(carol).online_peers().contains(&alice));
    assert!(expired_presence_for(&net, alice, carol));
    assert!(expired_presence_for(&net, carol, alice));
    assert!(expired_presence_for(&net, carol, bob));

    // The side that stayed together stayed usable throughout.
    assert!(net.peer(alice).online_peers().contains(&bob));
    net.peer(alice)
        .send_direct(bob, "still talking")
        .expect("the session survived the split");
    net.settle();
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["still talking"]
    );

    net.heal_partitions();
    net.run_for(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());

    // Presence recovers from evidence, with nothing to reconcile and nobody to
    // ask.
    assert!(net.peer(alice).online_peers().contains(&carol));
    assert!(net.peer(carol).online_peers().contains(&alice));

    net.peer(alice)
        .publish_broadcast("after the heal")
        .expect("gossip accepts it");
    net.settle();
    net.advance(gap_tolerance(&net));
    net.tick();

    // What was lost to the split stays lost — there is no history replay (AC10)
    // — and the range is named rather than forgotten (AC15).
    assert_eq!(
        net.peer(carol).transcript(ConversationId::Broadcast),
        ["before the split", "after the heal"]
    );
    let closed: Vec<_> = net
        .trace()
        .messaging_events_of(carol)
        .into_iter()
        .filter_map(|event| match event {
            MessagingEvent::MessageGapClosed(closed) => Some(closed),
            _ => None,
        })
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].author, alice);
    assert_eq!(closed[0].from.as_u64(), 2);
    assert_eq!(closed[0].to.as_u64(), 2);
    assert_eq!(closed[0].cause, GapCloseCause::ToleranceElapsed);
}
