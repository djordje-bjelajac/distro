//! The guard that makes every other file in this suite worth reading
//! (canvas AC13, safeguard S5).
//!
//! An integration suite over a simulated network is only evidence if the
//! simulation is reproducible. The scenario below exercises the same machinery
//! the rest of the suite asserts against — the bootstrap ladder including a
//! ticket, scrambled gossip, a duplicate, a forged signature, a local block, a
//! gap abandoned by the clock, a departure, and a restart — and pins the whole
//! run to its rendered trace.

use infra_sim_net::SimNetwork;

/// Two seeds, because determinism is a property of a run and not of one lucky
/// seed.
const SEEDS: [u64; 2] = [90_005, 7];

#[test]
fn a_multi_peer_scenario_renders_a_byte_identical_trace_across_two_runs() {
    let first = reference_run(SEEDS[0]);
    let second = reference_run(SEEDS[0]);

    assert_eq!(
        first, second,
        "two runs of one script diverged; this suite proves nothing about anything"
    );

    // A guard against the reference scenario quietly becoming trivial: an empty
    // trace compares equal to another empty trace.
    assert!(
        first.lines().count() > 50,
        "the reference scenario stopped exercising the system:\n{first}"
    );
    assert!(
        first.contains("message-gap-closed") && first.contains("peer-presence-expired"),
        "the reference scenario stopped covering the clock-driven sweeps:\n{first}"
    );
}

#[test]
fn the_same_holds_under_a_different_seed() {
    assert_eq!(reference_run(SEEDS[1]), reference_run(SEEDS[1]));
}

/// A scenario touching every moving part this suite depends on.
///
/// Deliberately not a helper shared with the other files: those assert on
/// behaviour, and this one asserts on reproducibility, so it must be free to
/// stay exhaustive without any test's readability depending on it.
fn reference_run(seed: u64) -> String {
    let mut net = SimNetwork::seeded(seed)
        .with_peers(["alice", "bob", "carol", "dave"])
        .build();
    let (alice, bob, carol, dave) = (
        net.peer_id("alice"),
        net.peer_id("bob"),
        net.peer_id("carol"),
        net.peer_id("dave"),
    );

    // Two peers meet on the LAN; a third is alone on its own segment and needs
    // the one human step D1 admits to.
    net.boot(alice);
    net.boot(bob);
    net.isolate_from_lan(carol);
    net.initialize(carol);
    let ticket = net.join_ticket_from(alice);
    net.peer(carol)
        .join_with_ticket(ticket)
        .expect("the publisher is healthy");
    net.settle();
    net.boot(dave);

    // Scrambled gossip from the seeded stream, with one message duplicated and
    // one signature corrupted in flight.
    let mut delays: Vec<u64> = (1..=9).map(|step| step * 7).collect();
    net.shuffle(&mut delays);
    net.script_delays(delays);
    net.duplicate_next(1);
    net.corrupt_next_signatures(1);
    for text in ["one", "two", "three"] {
        net.peer(alice)
            .publish_broadcast(text)
            .expect("gossip accepts it");
    }
    net.settle();

    // A local block, and a 1:1 exchange that outlives it.
    net.peer(dave).block(alice).expect("alice is not blocked");
    net.peer(bob)
        .send_direct(alice, "a direct word")
        .expect("the session is up");
    net.settle();

    // Both clock-driven sweeps: gaps left by the forgery and the block are
    // abandoned, and a departure nobody announced is noticed.
    net.advance(net.settings().gap_tolerance.as_millis());
    net.tick();
    net.stop(carol);
    net.run_for(70_000);

    // And a restart, which keeps the identity, the cache, and the counter, and
    // loses everything else.
    net.restart(bob);
    net.peer(bob).join().expect("the publisher is healthy");
    net.settle();
    net.peer(bob)
        .publish_broadcast("still here")
        .expect("gossip accepts it");
    net.settle();

    net.render_trace()
}
