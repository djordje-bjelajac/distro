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
        first.contains("peer-presence-expired"),
        "the reference scenario stopped covering the presence sweep:\n{first}"
    );

    // The gap sweep, and specifically the half of it that is still loss. Since
    // D10 a first sighting part-way through an author's run *establishes the
    // origin* and reports nothing, so a scenario can drift into covering only
    // that and leave the abandon-a-real-range path — the one AC15 is about —
    // untested while still containing the words `message-gap-closed`. A range
    // starting at 2 or above is a run between two sequences the log actually
    // observed, which is the only kind D10 still calls loss (canvas `0010` A2).
    let abandoned: Vec<&str> = first
        .lines()
        .filter(|line| line.contains("message-gap-closed"))
        .collect();
    assert!(
        abandoned
            .iter()
            .any(|line| line.contains("range=") && !line.contains("range=1..=")),
        "the reference scenario stopped covering a genuinely abandoned range; \
         gap closes seen: {abandoned:?}\n{first}"
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

    // One clean broadcast before anything is scrambled, so every listener's
    // stream has a floor.
    //
    // Without it the corrupted message below is the first sequence its
    // recipient ever sees from alice, which since D10 *establishes the origin*
    // rather than opening a gap — the sweep would then have nothing to abandon
    // and this scenario would quietly stop covering the clock-driven gap close
    // it exists to pin (canvas `0010` D10, A1/A2). The guard in the test above
    // fails loudly if that happens again.
    net.peer(alice)
        .publish_broadcast("before anything went wrong")
        .expect("gossip accepts it");
    net.settle();

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
