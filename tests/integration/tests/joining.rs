//! Joining, membership, and presence: what a peer does between being launched
//! and being part of a network (canvas AC1, AC2, AC3, AC5, AC9, invariant 3).
//!
//! Every scenario here runs over the deterministic simulated network. No test
//! reads a real clock, opens a socket, starts a thread, or draws from an
//! unseeded source: the only things that move are the ones a scenario moves
//! (AC13, safeguard S5).

use infra_sim_net::SimNetwork;
use membership::domain::events::MembershipEvent;
use membership::domain::{
    DurationMillis, JoinTicket, JoinTicketError, LivenessWindows, NetworkStatus, SessionDirection,
    SessionOutcome, SessionState,
};
use membership::ports::{BootstrapRung, DiscoveredPeer, RungFailure};
use messaging::domain::ConversationId;
use shared_types::{PeerId, ProtocolVersion};

/// One seed for the whole file, written down rather than picked implicitly: a
/// scenario's determinism is stated by its seed (AC13).
const SEED: u64 = 90_001;

/// A latency every link has, so elapsed virtual time in these scenarios means
/// something. Zero would make AC2's five-second bound vacuous.
const LAN_LATENCY_MILLIS: u64 = 20;

fn lan(labels: &[&str]) -> SimNetwork {
    let net = SimNetwork::seeded(SEED)
        .with_peers(labels.iter().copied())
        .build();
    net.set_default_delay(LAN_LATENCY_MILLIS);
    net
}

/// Connects every peer to every other, the way instances that have all seen
/// each other announce eventually do.
///
/// `boot_all` connects each peer to the *first* candidate that answers — enough
/// to join, not a mesh — and a scenario about departures needs every peer to
/// hold a link with the one that leaves.
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

// ---------------------------------------------------------------------------
// AC1, AC2, AC9 — launch and first contact
// ---------------------------------------------------------------------------

#[test]
fn two_unconfigured_peers_on_one_lan_discover_each_other_and_connect() {
    // AC1: first launch with no config, no args, and no prior state produces a
    // working identity and a listening node, with no registration step.
    // AC2: two instances on one LAN discover each other and connect within 5 s,
    // unconfigured.
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    let launched_at = net.now();

    // Before either has done anything: no identity, no peers, no network. This
    // is the state AC1 describes the "before" of, and nothing in it was
    // configured.
    for label in ["alice", "bob"] {
        assert!(
            net.named(label).local_identity().is_none(),
            "{label} had an identity before it was launched"
        );
        assert_eq!(net.named(label).network_status(), NetworkStatus::Isolated);
        assert!(net.named(label).known_peers().is_empty());
    }

    net.initialize(alice);
    net.initialize(bob);

    // The identity exists, and it is the one the peer keeps (AC9). Nothing was
    // asked of a user to produce it.
    let assumed = net
        .peer(alice)
        .local_identity()
        .expect("a launch assumes an identity");
    assert_eq!(assumed.peer, alice);

    // Alice is first up, so her ladder finds an empty LAN: `Isolated` is a
    // normal state, and she is listening and announced regardless.
    let alice_join = net.peer(alice).join().expect("the publisher is healthy");
    net.settle();
    assert!(!alice_join.succeeded());
    assert_eq!(alice_join.status, NetworkStatus::Isolated);

    let bob_join = net.peer(bob).join().expect("the publisher is healthy");
    net.settle();

    // Bob's LAN rung is what connected him: no cache to load, no ticket pasted,
    // and the ticket rung was never reached.
    assert!(bob_join.succeeded());
    assert_eq!(
        bob_join.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::NoCandidates),
        "a fresh install has no cache to bootstrap from"
    );
    assert_eq!(bob_join.diagnostic.connected_peer(), Some(alice));
    assert_eq!(
        bob_join.diagnostic.rungs_tried(),
        vec![BootstrapRung::CachedPeers, BootstrapRung::LocalNetwork],
        "the ticket rung must not be reached when the LAN answered"
    );

    // Both sides hold the session, and both say so.
    assert!(net.peer(alice).is_connected_to(bob));
    assert!(net.peer(bob).is_connected_to(alice));
    assert_eq!(
        net.peer(alice).network_status(),
        NetworkStatus::from_connected_peers(1)
    );
    assert_eq!(
        net.peer(bob).network_status(),
        NetworkStatus::from_connected_peers(1)
    );

    // AC2's bound, measured on the clock the scenario controls.
    let elapsed = net.now() - launched_at;
    assert!(
        elapsed < 5_000,
        "discovery and connection took {elapsed} ms, over AC2's five-second bound"
    );
}

// ---------------------------------------------------------------------------
// AC3 — the bootstrap ladder, including the rung that needs a human
// ---------------------------------------------------------------------------

#[test]
fn a_peer_with_no_cache_no_neighbour_and_no_ticket_ends_isolated_with_a_diagnostic() {
    // AC3: failure produces a visible diagnostic, never a hang. The call
    // returning at all is half the claim; the other half is that the diagnostic
    // names every rung that was tried and why each one gave nothing.
    let net = lan(&["alice", "bob"]);
    let alice = net.peer_id("alice");

    net.isolate_from_lan(alice);
    net.initialize(alice);

    let outcome = net.peer(alice).join().expect("the publisher is healthy");
    net.settle();

    assert!(!outcome.succeeded());
    assert_eq!(outcome.status, NetworkStatus::Isolated);
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        BootstrapRung::LADDER.to_vec(),
        "a failed join must have tried, and reported, every rung"
    );

    // Each rung states its own reason rather than a shared "failed".
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::NoCandidates)
    );
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::LocalNetwork),
        Some(RungFailure::NoCandidates)
    );
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::NoCandidates)
    );

    // And the whole thing is renderable: this is the text a user is shown
    // instead of a spinner.
    let rendered = outcome.diagnostic.to_string();
    for rung in BootstrapRung::LADDER {
        assert!(
            rendered.contains(&rung.to_string()),
            "the diagnostic never mentioned the {rung} rung: {rendered}"
        );
    }

    // Isolated is a state, not a failure: the peer is still listening, and it
    // still answers a dial.
    assert!(net.peer(alice).known_peers().is_empty());
}

#[test]
fn a_peer_with_no_lan_neighbour_joins_with_a_ticket_from_a_peer_it_cannot_see() {
    // AC3 and D1's third rung: one pasted string, produced by any member, and
    // the join crosses a LAN boundary that discovery cannot.
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot(alice);
    net.isolate_from_lan(bob);
    net.initialize(bob);

    // The LAN rung genuinely gives nothing here — proved by the failure the
    // same run records — so the ticket is what did the work.
    let ticket = net.join_ticket_from(alice);
    let outcome = net
        .peer(bob)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");
    net.settle();

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::LocalNetwork),
        Some(RungFailure::NoCandidates),
        "the LAN rung must have been tried and found empty"
    );
    assert_eq!(outcome.diagnostic.connected_peer(), Some(alice));
    assert!(net.peer(bob).is_connected_to(alice));
    assert!(net.peer(alice).is_connected_to(bob));
}

#[test]
fn an_expired_ticket_is_refused_with_an_expiry_error() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot(alice);
    net.isolate_from_lan(bob);
    net.initialize(bob);

    let ticket = net.join_ticket_expiring_after(alice, DurationMillis::ZERO);
    let outcome = net
        .peer(bob)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");
    net.settle();

    assert!(!outcome.succeeded());
    assert_eq!(outcome.status, NetworkStatus::Isolated);
    assert!(
        matches!(
            outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
            Some(RungFailure::Ticket(JoinTicketError::Expired { .. }))
        ),
        "an expired ticket must fail as expired, not as unreachable: {:?}",
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket)
    );
    assert!(!net.peer(bob).is_connected_to(alice));
}

#[test]
fn a_ticket_from_an_unsupported_major_version_is_refused_with_a_version_error() {
    // S2: peers upgrade independently, so a ticket minted by a build speaking a
    // different major version is refused with that stated as the reason, rather
    // than being dialled and failing obscurely later.
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot(alice);
    net.isolate_from_lan(bob);
    net.initialize(bob);

    let next_major = ProtocolVersion::new(ProtocolVersion::CURRENT.major + 1, 0);
    let ticket = JoinTicket::expiring_after(
        alice,
        net.fabric().endpoints_of(alice),
        next_major,
        membership::domain::Millis::from_millis(net.now()),
        JoinTicket::DEFAULT_LIFETIME,
    )
    .expect("a registered peer always has at least one endpoint");

    let outcome = net
        .peer(bob)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");
    net.settle();

    assert!(!outcome.succeeded());
    assert!(
        matches!(
            outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
            Some(RungFailure::Ticket(
                JoinTicketError::IncompatibleProtocol { .. }
            ))
        ),
        "a wrong-major ticket must be refused as incompatible: {:?}",
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket)
    );
    assert!(!net.peer(bob).is_connected_to(alice));
}

#[test]
fn a_restart_with_a_warm_cache_rejoins_without_a_ticket() {
    // D1's promise that a ticket is a one-time cost: the cache rung is the
    // first one tried, and here it is the only one that can work.
    let mut net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.boot_all();
    net.peer(alice).leave().expect("the publisher is healthy");
    net.settle();
    assert!(net.peer(alice).durable().cache().holds(bob));

    net.restart(alice);

    // AC9: the identity across the restart is the one it had, assumed without
    // anything being asked of a user. A peer that came back as somebody else
    // would rejoin just as well and prove nothing.
    assert_eq!(
        net.peer(alice).local_identity().expect("assumed").peer,
        alice
    );

    // No neighbour to discover and no ticket to redeem: if the peer rejoins, it
    // rejoined from what it remembered.
    net.isolate_from_lan(alice);
    let outcome = net.peer(alice).join().expect("the publisher is healthy");
    net.settle();

    assert!(outcome.succeeded());
    assert_eq!(outcome.diagnostic.connected_peer(), Some(bob));
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        vec![BootstrapRung::CachedPeers],
        "the cache answered, so no further rung should have been walked"
    );
    assert!(net.peer(alice).is_connected_to(bob));
    assert!(net.peer(bob).is_connected_to(alice));
}

// ---------------------------------------------------------------------------
// Invariant 3 — simultaneous connect
// ---------------------------------------------------------------------------

#[test]
fn a_simultaneous_connect_collapses_to_the_one_session_both_sides_agree_on() {
    // Invariant 3: the session initiated by the lexicographically lower `PeerId`
    // survives, and both sides compute that identically. This is the normal
    // case, not an edge case, so it is staged as one: both peers dial before
    // anything is delivered.
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));

    net.initialize(alice);
    net.initialize(bob);

    for (peer, other) in [(alice, bob), (bob, alice)] {
        net.peer(peer)
            .peer_observed(DiscoveredPeer {
                peer: other,
                endpoints: net.fabric().endpoints_of(other),
            })
            .expect("the roster accepts a discovery");
    }

    // Both dials happen with nothing pumped in between, so neither peer knows
    // the other is dialling it.
    net.peer(alice).connect_to(bob).expect("bob answers");
    net.peer(bob).connect_to(alice).expect("alice answers");

    // Each side now takes the other's dial in. This is the same inbound port
    // call the pump makes on delivery, invoked directly so that the collapse
    // each side computed is readable rather than discarded.
    let outcomes: Vec<(PeerId, SessionOutcome)> = [(alice, bob), (bob, alice)]
        .into_iter()
        .map(|(peer, other)| {
            let outcome = net
                .peer(peer)
                .session_opened(other, net.fabric().endpoints_of(other))
                .expect("the roster takes an inbound dial");
            (peer, outcome)
        })
        .collect();

    let at_alice = &outcomes[0].1;
    let at_bob = &outcomes[1].1;
    let alice_collapse = at_alice
        .collapse
        .expect("alice was dialled while dialling: that is a collapse");
    let bob_collapse = at_bob
        .collapse
        .expect("bob was dialled while dialling: that is a collapse");

    let lower = alice.min(bob);
    let higher = alice.max(bob);

    // Both sides name the same initiator, computed locally from the same rule.
    assert_eq!(alice_collapse.initiator(), lower);
    assert_eq!(bob_collapse.initiator(), lower);

    // The two views are mirror images of one session: what is outbound at one
    // end is inbound at the other, and each side superseded the other one.
    assert_eq!(
        alice_collapse.survivor(),
        bob_collapse.survivor().opposite()
    );
    assert_eq!(
        at_alice.superseded,
        Some(alice_collapse.survivor().opposite())
    );
    assert_eq!(at_bob.superseded, Some(bob_collapse.survivor().opposite()));

    // Stated without reference to which name sorted first: the peer whose own
    // dial survived is the lower one, and the higher one adopted the dial it
    // received.
    let kept_its_own_dial = if alice_collapse.survivor() == SessionDirection::Outbound {
        alice
    } else {
        bob
    };
    assert_eq!(kept_its_own_dial, lower);

    // Exactly one session remains on each side.
    for peer in [alice, bob] {
        let known = net.peer(peer).known_peers();
        assert_eq!(known.len(), 1, "a peer pair produced more than one entry");
        assert!(
            known[0].session.is_some(),
            "the collapse left no session at all"
        );
    }

    // The survivor is the one the lower peer dialled: it is established at that
    // end already, and the higher peer is completing the handshake for it
    // rather than for the dial it just abandoned.
    assert!(net.peer(lower).is_connected_to(higher));
    assert_eq!(
        net.peer(higher).known_peers()[0].session,
        Some(SessionState::Connecting)
    );
}

// ---------------------------------------------------------------------------
// AC5 — a departure is observed, and costs nobody else anything
// ---------------------------------------------------------------------------

#[test]
fn stopping_one_peer_leaves_the_others_working_and_its_departure_is_observed() {
    // AC5: stopping any instance leaves all others functional; peers observe
    // the departure within the liveness window. Carol's process dies abruptly —
    // nothing is announced — so the only way anyone learns is by the evidence
    // of life going stale (invariant 7).
    let net = lan(&["alice", "bob", "carol"]);
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );

    net.boot_all();
    connect_every_pair(&net);

    for peer in [alice, bob] {
        assert!(net.peer(peer).online_peers().contains(&carol));
    }

    net.stop(carol);
    let stopped_at = net.now();

    while net.now() - stopped_at < LivenessWindows::DEFAULT_OFFLINE.as_millis() {
        net.run_for(LivenessWindows::HEARTBEAT_INTERVAL.as_millis());

        // "Leaves all others functional" is a claim about the whole interval,
        // not only its end.
        assert!(net.peer(alice).online_peers().contains(&bob));
        assert!(net.peer(bob).online_peers().contains(&alice));
    }

    let noticed_after = net.now() - stopped_at;
    assert!(
        noticed_after <= LivenessWindows::DEFAULT_OFFLINE.as_millis(),
        "the departure took {noticed_after} ms to notice, past the liveness window"
    );

    for peer in [alice, bob] {
        assert!(
            !net.peer(peer).online_peers().contains(&carol),
            "{} still counts the stopped peer as online",
            net.label_of(peer)
        );
        assert!(
            net.trace()
                .membership_events_of(peer)
                .iter()
                .any(|event| matches!(
                    event,
                    MembershipEvent::PeerPresenceExpired(expired) if expired.peer == carol
                )),
            "{} never reported the departure",
            net.label_of(peer)
        );
    }

    // And the survivors are not merely alive but usable.
    net.peer(alice)
        .send_direct(bob, "still here")
        .expect("the session outlived the third peer");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["still here"]
    );
}
