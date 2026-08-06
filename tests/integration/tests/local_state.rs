//! Throwing local state away: what forgetting cached peers and clearing chats
//! do to a running peer, and — the part no single context can claim on its own
//! — what they leave alone (canvas `0013`, A1–A6).
//!
//! Every scenario runs over the deterministic simulated network. No test reads
//! a real clock, opens a socket, starts a thread, or draws from an unseeded
//! source (AC13, safeguard S5).

use identity::domain::VerificationState;
use infra_sim_net::SimNetwork;
use membership::domain::NetworkStatus;
use membership::ports::{BootstrapRung, DiscoveredPeer};
use messaging::domain::ConversationId;
use shared_types::PeerId;

/// One seed for the whole file, written down rather than picked implicitly.
const SEED: u64 = 90_013;

const LAN_LATENCY_MILLIS: u64 = 20;

fn lan(labels: &[&str]) -> SimNetwork {
    let net = SimNetwork::seeded(SEED)
        .with_peers(labels.iter().copied())
        .build();
    net.set_default_delay(LAN_LATENCY_MILLIS);
    net
}

/// Introduces `one` to `other` and connects them, the way two instances that
/// have announced on the same link end up.
fn introduce(net: &SimNetwork, one: PeerId, other: PeerId) {
    net.peer(one)
        .peer_observed(DiscoveredPeer {
            peer: other,
            endpoints: net.fabric().endpoints_of(other),
        })
        .expect("the roster accepts a discovery");
    net.peer(one)
        .connect_to(other)
        .expect("an online peer answers");
    net.settle();
}

// ------------------------------------------------------- forgetting peers

/// A1. The roster and the file agree that there is nothing left — and the file
/// is not quietly repopulated by the leave that a quit performs.
#[test]
fn forgetting_empties_the_roster_and_the_cache_and_a_later_quit_keeps_it_empty() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    assert!(
        net.peer(alice).durable().cache().holds(bob) || !net.peer(alice).known_peers().is_empty(),
        "the scenario needs alice to know bob before she forgets him"
    );

    let outcome = net.peer(alice).forget_peers().expect("forgetting succeeds");

    assert_eq!(outcome.forgotten, 1);
    assert!(net.peer(alice).known_peers().is_empty());
    assert!(net.peer(alice).durable().cache().peers().is_empty());

    // The quit. This is the half a cache-only implementation fails: leaving
    // writes the cache from the roster, and a roster nobody emptied puts every
    // forgotten peer straight back on disk.
    net.peer(alice).leave().expect("leaving succeeds");

    assert!(
        net.peer(alice).durable().cache().peers().is_empty(),
        "quitting after a forget must not resurrect the peers"
    );
}

/// A2. What "cold start" means, checked at the rung that would have skipped it.
#[test]
fn a_peer_that_forgot_rejoins_from_a_cold_start_rather_than_from_its_cache() {
    let mut net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);

    net.peer(alice).forget_peers().expect("forgetting succeeds");
    // A restart is what makes the claim about the *next launch* rather than
    // about this process: the rebuilt peer has only what survived on disk.
    net.restart(alice);
    let outcome = net.peer(alice).join().expect("joining succeeds");

    assert!(
        outcome
            .diagnostic
            .rungs_tried()
            .contains(&BootstrapRung::CachedPeers),
        "the ladder still starts at the cached rung"
    );
    assert!(
        outcome
            .diagnostic
            .failure_of(BootstrapRung::CachedPeers)
            .is_some(),
        "and that rung had nothing to try: {:?}",
        outcome.diagnostic
    );
}

/// A3. Forgetting a peer is not a reason to stop blocking it — the security
/// regression this is most likely to be mistaken for.
#[test]
fn forgetting_peers_leaves_every_trust_decision_standing() {
    let net = lan(&["alice", "bob", "carol"]);
    let (alice, bob, carol) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
    );
    introduce(&net, alice, bob);
    introduce(&net, alice, carol);
    net.peer(alice).verify(bob).expect("verification records");
    net.peer(alice).block(carol).expect("blocking records");

    net.peer(alice).forget_peers().expect("forgetting succeeds");

    assert_eq!(
        net.peer(alice).blocked_peers().expect("trust reads"),
        vec![carol],
        "a blocked peer stays blocked"
    );
    assert_eq!(
        net.peer(alice)
            .trust_state(bob)
            .expect("trust reads")
            .verification,
        VerificationState::Verified,
        "a verified peer stays verified"
    );
}

/// A4. The identity and its place in every stream survive — the two things
/// that would make a "forget my peers" quietly destroy far more than it said.
#[test]
fn forgetting_peers_changes_neither_the_identity_nor_the_outbound_sequence() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    net.peer(alice)
        .publish_broadcast("before")
        .expect("published");
    let mark_before = net
        .peer(alice)
        .durable()
        .counter()
        .mark(ConversationId::Broadcast);
    assert!(mark_before.is_some(), "alice has issued a broadcast mark");

    net.peer(alice).forget_peers().expect("forgetting succeeds");

    assert_eq!(net.peer_id("alice"), alice, "the identity is unchanged");
    assert_eq!(
        net.peer(alice)
            .durable()
            .counter()
            .mark(ConversationId::Broadcast),
        mark_before,
        "the outbound mark did not move"
    );
}

/// Forgetting leaves the network, which peers observe as a departure rather
/// than as silence. Nothing about clearing local state is private to the peer
/// doing it once sessions are involved, and that is worth stating.
#[test]
fn forgetting_looks_like_a_departure_to_everyone_else() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    assert!(net.peer(bob).is_connected_to(alice));

    net.peer(alice).forget_peers().expect("forgetting succeeds");
    net.settle();

    assert!(!net.peer(bob).is_connected_to(alice));
    assert_eq!(net.peer(alice).network_status(), NetworkStatus::Isolated);
}

// -------------------------------------------------------- clearing chats

/// A5, across contexts: the screen empties, and stays empty.
#[test]
fn clearing_empties_every_conversation_this_peer_holds() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    net.peer(alice).publish_broadcast("everyone").expect("sent");
    net.peer(alice).send_direct(bob, "just you").expect("sent");
    net.settle();

    let cleared = net.peer(alice).clear_history().expect("clearing succeeds");

    assert!(cleared.messages_dropped >= 2);
    assert!(net.peer(alice).conversations().is_empty());
    assert!(net.peer(alice).broadcast_history().is_empty());
    assert!(net.peer(alice).direct_history(bob).is_empty());
}

/// A6, and the whole reason clearing is safe: `bob` was listening before the
/// clear and is still listening after it. A peer whose counter had been reset
/// would have every later message classified a duplicate — going mute while
/// its own screen looked healthy.
#[test]
fn a_peer_that_cleared_its_history_is_still_heard_by_one_that_did_not() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    net.peer(alice).send_direct(bob, "first").expect("sent");
    net.settle();
    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["first"]
    );

    net.peer(alice).clear_history().expect("clearing succeeds");
    net.peer(alice).send_direct(bob, "second").expect("sent");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["first", "second"],
        "bob applied the message sent after alice cleared, so her sequence \
         kept climbing rather than restarting"
    );
}

/// Clearing is local. The peer on the other side keeps what it received, which
/// is what makes this a "clear my copy" rather than an unsend.
#[test]
fn clearing_takes_nothing_from_the_peer_on_the_other_side() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    net.peer(alice)
        .send_direct(bob, "remember this")
        .expect("sent");
    net.settle();

    net.peer(alice).clear_history().expect("clearing succeeds");
    net.settle();

    assert_eq!(
        net.peer(bob).transcript(ConversationId::Direct(alice)),
        ["remember this"]
    );
}

/// The two operations are independent, and each says so by leaving the other's
/// state alone. Run together they are still just the two of them.
#[test]
fn clearing_keeps_the_roster_and_forgetting_keeps_the_history() {
    let net = lan(&["alice", "bob"]);
    let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
    introduce(&net, alice, bob);
    net.peer(alice).publish_broadcast("said").expect("sent");
    net.settle();

    net.peer(alice).clear_history().expect("clearing succeeds");

    assert!(
        !net.peer(alice).known_peers().is_empty(),
        "clearing chats is not a reason to forget who is out there"
    );

    net.peer(alice)
        .publish_broadcast("said again")
        .expect("sent");
    net.peer(alice).forget_peers().expect("forgetting succeeds");

    assert_eq!(
        net.peer(alice).broadcast_history().len(),
        1,
        "forgetting peers is not a reason to clear the screen"
    );
}
