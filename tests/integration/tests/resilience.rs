//! What happens when the network itself misbehaves: a path that cannot be
//! dialled, a network that splits in two, a link that outlives the peer behind
//! it, and an address that is announced and never answers (canvas AC5, AC12,
//! safeguard S7; canvas `0010` A3b, D3, D4, D5).

use std::sync::Arc;

use infra_sim_net::{DialFault, SimNetwork, SimPeerTransport};
use membership::domain::events::MembershipEvent;
use membership::domain::{Endpoint, LivenessWindows, PeerStanding, Presence, Reachability};
use membership::ports::{
    BootstrapRung, DiscoveredPeer, KnownPeerView, NetworkView, PeerTransportError,
    PeerTransportPort, RungFailure,
};
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

/// Two peers and nobody else.
///
/// The fabric routes a severed link around any third peer that can reach both
/// ends — that is AC12, and it is exactly what the two scenarios at the foot of
/// this file must not have. With nobody to relay, a cut path stays cut and what
/// the local view says is a statement about silence rather than about a detour.
fn pair() -> SimNetwork {
    SimNetwork::seeded(SEED)
        .with_peers(["alice", "bob"])
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

/// One peer's row in a snapshot of somebody's screen.
///
/// Taken from the snapshot rather than looked up again, because a second
/// lookup is a second reading: the whole of canvas `0010` D5 is that the count
/// and the row it counts come from one traversal, and a test that re-read the
/// roster to find the row would be asserting on a pair the code never produced
/// together.
fn row_of(view: &NetworkView, peer: PeerId) -> &KnownPeerView {
    view.peers()
        .iter()
        .find(|row| row.peer == peer)
        .unwrap_or_else(|| panic!("{peer:?} has no row in this snapshot"))
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

// ---------------------------------------------------------------------------
// Canvas `0010` — the two states the observed screen could not say honestly
//
// # What these two tests are, and what they are not (safeguard S7)
//
// They pin the *pieces*. `infra-sim-net` has no gossip mesh and no mDNS, so the
// composite three-instance failure that produced the screenshots is **not
// reproducible here**, and nothing below should be read as covering it — its
// re-verification is manual, alongside the still-unrun two-machine smoke. What
// is genuinely exercised is every layer above the discovery and transport
// ports: the roster, presence derivation, the standing, the one-snapshot query,
// the bootstrap ladder, and the peer cache. That is where the fabricated
// evidence was.
// ---------------------------------------------------------------------------

#[test]
fn a_link_whose_heartbeats_are_all_dropped_reads_offline_and_stays_counted() {
    // The screenshot's state, rendered honestly: `connected (1 peer)` above a
    // row that says the peer is not answering, which is neither a contradiction
    // nor a race but two independently true facts about one peer (canvas `0010`
    // D4, D5).
    //
    // The point of the test is the pair of shortcuts it forbids (safeguard S4).
    // Dropping the peer from `Connected(n)` to make the screen agree with
    // itself would hide a link a direct message can still be attempted over;
    // deriving `Online` from the fact that the session is up would be the same
    // fabricated evidence this canvas removed, in a new place. Both would
    // satisfy a naive reading of "the two must not contradict each other", and
    // both are asserted against here.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot_all();

    let before = net.peer(alice).network_view();
    assert_eq!(before.status().connected_peers(), 1);
    assert_eq!(
        row_of(&before, bob).standing(),
        PeerStanding::Linked(Presence::Online),
        "the pair did not start from a healthy link"
    );

    // Nothing closes the session — neither side is told anything — and every
    // frame over it is dropped. That is what a link to a peer that stopped
    // speaking looks like: there is no ping behaviour in this build, so a
    // socket to a dead process sits established indefinitely (invariant 3).
    net.sever_link(alice, bob);

    let severed_at = net.now();
    while net.now() - severed_at < LivenessWindows::DEFAULT_OFFLINE.as_millis() {
        net.run_for(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());
    }

    let view = net.peer(alice).network_view();
    let row = row_of(&view, bob);

    // Both halves, from one snapshot.
    assert_eq!(row.standing(), PeerStanding::Linked(Presence::Offline));
    assert_eq!(row.presence, Presence::Offline);
    assert!(row.is_connected(), "the session was closed by something");
    assert_eq!(
        view.status().connected_peers(),
        1,
        "the working link was suppressed to make the screen agree"
    );

    // And the count really is read off these rows rather than checked against
    // them: one traversal, one classification, so a disagreement between the
    // status line and the roster would have to be an arithmetic error.
    assert_eq!(
        view.standings()
            .iter()
            .filter(|standing| standing.is_linked())
            .count(),
        view.status().connected_peers()
    );

    // Nothing anywhere claims the peer is alive, and the expiry fired because
    // bob had produced evidence and then stopped — which is the only case
    // `PeerPresenceExpired` has an honest `last_evidence_at` for (invariant 5).
    assert!(net.peer(alice).online_peers().is_empty());
    assert!(expired_presence_for(&net, alice, bob));

    // Each view is authoritative only for itself (invariant 9), and the cut is
    // symmetric, so bob says the same of alice.
    let from_bob = net.peer(bob).network_view();
    assert_eq!(
        row_of(&from_bob, alice).standing(),
        PeerStanding::Linked(Presence::Offline)
    );
    assert_eq!(from_bob.status().connected_peers(), 1);

    // Nothing latched: presence recovers from evidence and from nothing else,
    // so one heartbeat that gets through is enough.
    net.restore_link(alice, bob);
    net.run_for(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());

    let healed = net.peer(alice).network_view();
    assert_eq!(
        row_of(&healed, bob).standing(),
        PeerStanding::Linked(Presence::Online)
    );
    assert_eq!(healed.status().connected_peers(), 1);
}

#[test]
fn an_announced_peer_that_never_answers_stays_unknown_through_every_re_announcement() {
    // A3b at multi-peer level: `record_discovery` takes no evidence instant, so
    // neither the first sighting nor the eighth can make a peer look alive
    // (canvas `0010` D3, safeguard S2). The domain states this at the
    // aggregate; this states it through the path a hostile record actually
    // travels — `PeerDiscoveryPort`, the ladder's LAN rung, the roster, the
    // one-snapshot query, and finally the peer cache.
    //
    // Bob here is any address a third party publishes: announced on the LAN,
    // entered in the roster because it is a dialable candidate, and never once
    // heard from.
    let net = pair();
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot(bob);
    net.set_dial_fault(alice, bob, DialFault::Unreachable);
    net.initialize(alice);

    let started_at = net.now();

    // Eight joins over eighty seconds of virtual time: more re-announcements
    // than any ladder needs, and more elapsed time than the offline window, so
    // an `Unknown` that were secretly a rung on the ageing ladder would have
    // reached `Offline` well before the last round (invariant 4).
    for round in 1..=8 {
        let outcome = net.peer(alice).join().expect("the publisher is healthy");

        // The re-announcement genuinely reached the LAN rung and was genuinely
        // dialled. Without this the rest of the loop would pass just as well on
        // a LAN that had gone quiet, which is the way this test could rot into
        // proving nothing.
        assert_eq!(
            outcome.diagnostic.failure_of(BootstrapRung::LocalNetwork),
            Some(RungFailure::Unreachable { candidates: 1 }),
            "round {round}: bob was not observed and dialled"
        );
        assert!(outcome.status.is_isolated(), "round {round}");

        net.settle();
        net.advance(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());
        net.tick();
        net.mdns_tick();

        let view = net.peer(alice).network_view();
        let row = row_of(&view, bob);

        assert_eq!(
            row.standing(),
            PeerStanding::Unlinked(Presence::Unknown),
            "round {round}: a sighting was treated as evidence"
        );
        assert_eq!(row.last_seen_at, None, "round {round}");
        assert_eq!(row.session, None, "round {round}");
        assert!(view.status().is_isolated(), "round {round}");
        assert!(net.peer(alice).online_peers().is_empty(), "round {round}");
    }

    assert!(
        net.now() - started_at > LivenessWindows::DEFAULT_OFFLINE.as_millis(),
        "the loop did not outlast the offline window, so nothing was proved \
         about ageing"
    );

    // `Unknown` has exactly one exit and time is not it: a peer that never
    // spoke cannot have gone away, so nothing was announced as expired and the
    // negative verdict was never reached (invariants 4 and 5).
    assert!(!expired_presence_for(&net, alice, bob));

    // The row is still there. Never-heard-from peers are shown rather than
    // hidden — they are dialable candidates, and hiding them turns "my peer
    // vanished" into a support question (canvas `0010` §3).
    assert!(
        net.peer(alice)
            .known_peers()
            .iter()
            .any(|row| row.peer == bob)
    );

    // And nothing about bob survives the process. The cache is read back by the
    // **first** rung of the next launch's ladder and dialled ahead of the LAN,
    // so an identity this peer was merely told about must not reach it (canvas
    // `0010` D8, safeguard S5).
    let left = net.peer(alice).leave().expect("the publisher is healthy");
    assert_eq!(left.cached_peers, 0);
    assert!(!net.peer(alice).durable().cache().holds(bob));
}
