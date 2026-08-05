use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId as Libp2pPeerId};
use membership::domain::Endpoint;

use crate::limits::ResourceLimits;
use crate::mapping::EndpointMapping;
use crate::swarm::external_address_ledger::CORROBORATION_THRESHOLD;
use crate::swarm::reachability_ledger::{
    ProbeOutcome, ProbeResult, Reachability, ReachabilityLedger,
};

/// A globally routable address, from RFC 5737's documentation range so nothing
/// here can be mistaken for a real host.
const PUBLIC: &str = "/ip4/203.0.113.7/tcp/4001";

/// A second one, for the tests that need two addresses to be different.
const OTHER: &str = "/ip4/203.0.113.8/tcp/4001";

fn endpoint(text: &str) -> Endpoint {
    EndpointMapping::parse(text).expect("a well-formed multiaddress")
}

/// A distinct, deterministic AutoNAT server per seed.
///
/// Derived from a keypair rather than randomised so a failing run can be re-run
/// and say the same thing (AC13).
fn server(seed: u8) -> Libp2pPeerId {
    Keypair::ed25519_from_bytes([seed; 32])
        .expect("32 bytes are a valid Ed25519 seed")
        .public()
        .to_peer_id()
}

fn ledger() -> ReachabilityLedger {
    ReachabilityLedger::new(ResourceLimits::DEFAULT.max_failing_addresses)
}

/// The `n`th address of a flood, well-formed and globally routable.
fn flooded(index: usize) -> Multiaddr {
    format!("/ip4/203.0.113.7/tcp/{}", 4000 + index)
        .parse()
        .expect("a well-formed multiaddress")
}

#[test]
fn a_new_ledger_is_unknown_and_unknown_is_never_unreachable() {
    // P2-3 and S3, which is the invariant every other test here leans on: a
    // peer that has not been probed has not been *refused*. Collapsing the two
    // would have every peer claim, for the first seconds of every launch, the
    // one thing this piece must never say falsely.
    let ledger = ledger();

    assert_eq!(ledger.reachability(), &Reachability::Unknown);
    assert_ne!(Reachability::Unknown, Reachability::Unreachable);
    assert_ne!(
        Reachability::Unknown,
        Reachability::Reachable(endpoint(PUBLIC))
    );
}

#[test]
fn one_successful_probe_makes_the_peer_reachable_at_that_address() {
    // P2-1 and invariant 1's forgiving half. A dial-back that arrived is proof:
    // a server cannot fake a connection this peer's own transport accepted, and
    // no attacker gains anything by convincing us we are reachable.
    let mut ledger = ledger();

    assert_eq!(
        ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Succeeded),
        ProbeOutcome::Changed(Reachability::Reachable(endpoint(PUBLIC)))
    );
    assert_eq!(
        ledger.reachability(),
        &Reachability::Reachable(endpoint(PUBLIC))
    );
}

#[test]
fn one_failed_probe_leaves_the_peer_unknown() {
    // **The asymmetry (P2-4, D2, S2), and the most important assertion in this
    // file.** A failure is one server's word. That server may be broken,
    // overloaded, or hostile, and telling a reachable user "you are
    // unreachable" sends them to change router settings that were never wrong —
    // a worse outcome than saying nothing at all.
    let mut ledger = ledger();

    assert_eq!(
        ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed),
        ProbeOutcome::Unchanged,
        "a single failure changes nothing a user is shown"
    );
    assert_eq!(
        ledger.reachability(),
        &Reachability::Unknown,
        "still Unknown — not Unreachable (S3)"
    );
}

#[test]
fn two_distinct_servers_failing_the_same_address_conclude_unreachable() {
    // P2-2 and invariant 2. Evidence is kept, and the second *distinct* server
    // is what turns it into a verdict.
    let mut ledger = ledger();

    ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed);

    assert_eq!(
        ledger.record(server(2), endpoint(PUBLIC), ProbeResult::Failed),
        ProbeOutcome::Changed(Reachability::Unreachable)
    );
    assert_eq!(ledger.reachability(), &Reachability::Unreachable);
}

#[test]
fn one_server_failing_forever_never_concludes_unreachable() {
    // Invariant 2's teeth. If corroboration counted *observations* rather than
    // *distinct servers*, one broken or hostile peer would meet any threshold
    // by itself simply by being asked twice — which is the whole attack.
    let mut ledger = ledger();

    for _ in 0..64 {
        assert_eq!(
            ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed),
            ProbeOutcome::Unchanged
        );
    }

    assert_eq!(ledger.reachability(), &Reachability::Unknown);
}

#[test]
fn unreachable_requires_exactly_the_corroboration_threshold_piece_one_uses() {
    // D2: one story about not trusting a single peer, one constant. This test
    // fails the day somebody defines a second threshold here rather than
    // importing piece 1's.
    let mut ledger = ledger();

    for seed in 1..CORROBORATION_THRESHOLD {
        let seed = u8::try_from(seed).expect("the threshold is small");
        assert_eq!(
            ledger.record(server(seed), endpoint(PUBLIC), ProbeResult::Failed),
            ProbeOutcome::Unchanged,
            "{seed} distinct servers are below the threshold"
        );
        assert_eq!(ledger.reachability(), &Reachability::Unknown);
    }

    let last = u8::try_from(CORROBORATION_THRESHOLD).expect("the threshold is small");
    assert_eq!(
        ledger.record(server(last), endpoint(PUBLIC), ProbeResult::Failed),
        ProbeOutcome::Changed(Reachability::Unreachable)
    );
}

#[test]
fn a_success_after_unreachable_returns_to_reachable_and_clears_the_evidence() {
    // P2-7 and invariant 4: the state is not a one-way latch. A peer whose
    // router was reconfigured, or whose laptop moved back onto a network with a
    // public address, must be able to say so — and the failures that condemned
    // it are stale the moment a dial-back arrives.
    let mut ledger = ledger();
    ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed);
    ledger.record(server(2), endpoint(PUBLIC), ProbeResult::Failed);
    assert_eq!(ledger.reachability(), &Reachability::Unreachable);

    assert_eq!(
        ledger.record(server(3), endpoint(PUBLIC), ProbeResult::Succeeded),
        ProbeOutcome::Changed(Reachability::Reachable(endpoint(PUBLIC)))
    );
    assert_eq!(
        ledger.failing_addresses(),
        0,
        "the evidence that condemned the address is cleared, not merely outvoted"
    );

    // And the proof that it is cleared rather than hidden: one more failure
    // from one server is back to being a single server's word.
    assert_eq!(
        ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed),
        ProbeOutcome::Unchanged
    );
    assert_eq!(
        ledger.reachability(),
        &Reachability::Reachable(endpoint(PUBLIC))
    );
}

#[test]
fn a_repeated_identical_verdict_is_unchanged() {
    // Invariant 5. AutoNAT re-probes on a timer, so the steady state of a
    // healthy peer is the same verdict over and over; waking the composition
    // root for each of them would make the status line flicker and the event
    // queue work for nothing.
    let mut ledger = ledger();

    assert_eq!(
        ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Succeeded),
        ProbeOutcome::Changed(Reachability::Reachable(endpoint(PUBLIC)))
    );
    for seed in 1..=8 {
        assert_eq!(
            ledger.record(server(seed), endpoint(PUBLIC), ProbeResult::Succeeded),
            ProbeOutcome::Unchanged
        );
    }
}

#[test]
fn a_corroborated_failure_of_the_reachable_address_makes_the_peer_unreachable() {
    // The canvas §9 scenario: a peer that was reachable moves onto a network
    // where it is not. Its previously confirmed address is never retracted —
    // that is the recorded standing follow-up — but the verdict it reports must
    // follow the evidence rather than stay stuck on the good news.
    let mut ledger = ledger();
    ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Succeeded);

    ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Failed);
    assert_eq!(
        ledger.reachability(),
        &Reachability::Reachable(endpoint(PUBLIC)),
        "one server's failure does not overturn proof (S2)"
    );

    assert_eq!(
        ledger.record(server(2), endpoint(PUBLIC), ProbeResult::Failed),
        ProbeOutcome::Changed(Reachability::Unreachable)
    );
}

#[test]
fn a_proven_address_survives_corroborated_failures_of_a_different_address() {
    // S2 in its sharpest form. A multi-homed peer whose IPv4 path is blocked
    // and whose IPv6 path works is *reachable*: strangers can dial it. Reporting
    // `Unreachable` because one of its addresses fails would be precisely the
    // false negative this piece exists to avoid.
    let mut ledger = ledger();
    ledger.record(server(1), endpoint(PUBLIC), ProbeResult::Succeeded);

    ledger.record(server(1), endpoint(OTHER), ProbeResult::Failed);
    assert_eq!(
        ledger.record(server(2), endpoint(OTHER), ProbeResult::Failed),
        ProbeOutcome::Unchanged
    );
    assert_eq!(
        ledger.reachability(),
        &Reachability::Reachable(endpoint(PUBLIC)),
        "an address that works outranks an address that does not"
    );
}

#[test]
fn the_failure_evidence_bound_holds_under_a_flood() {
    // Invariant 6 and S6. Probe results are produced by servers this peer did
    // not choose, about addresses fed in by whatever the swarm saw, so the
    // structure that holds them is attacker-influenced like every other one at
    // this boundary and refuses rather than grows.
    let cap = ResourceLimits::DEFAULT.max_failing_addresses;
    let mut ledger = ledger();

    for index in 0..cap * 4 {
        let address = EndpointMapping::to_endpoint(&flooded(index)).expect("a routable address");
        assert_eq!(
            ledger.record(server(1), address, ProbeResult::Failed),
            ProbeOutcome::Unchanged
        );
    }

    assert_eq!(ledger.failing_addresses(), cap);
    assert_eq!(
        ledger.reachability(),
        &Reachability::Unknown,
        "a flood from one server corroborates nothing, however large it is"
    );

    // A full ledger still corroborates what it already holds: the bound refuses
    // *new* addresses, it does not stop evidence from completing.
    let held = EndpointMapping::to_endpoint(&flooded(0)).expect("a routable address");
    assert_eq!(
        ledger.record(server(2), held, ProbeResult::Failed),
        ProbeOutcome::Changed(Reachability::Unreachable)
    );
    assert_eq!(ledger.failing_addresses(), cap);

    // And an address the bound refused can never be condemned, however many
    // servers report it: nothing about it was recorded in the first place.
    let refused = EndpointMapping::to_endpoint(&flooded(cap * 4)).expect("a routable address");
    for seed in 1..=8 {
        ledger.record(server(seed), refused.clone(), ProbeResult::Failed);
    }
    assert_eq!(ledger.failing_addresses(), cap);
}

#[test]
fn evidence_for_one_address_stops_accumulating_once_it_is_condemned() {
    // The second half of the bound, and the reason one number is enough: an
    // address is condemned at the threshold and takes no further evidence, so
    // the servers held per address are capped by the threshold itself. An
    // endless crowd of fresh identities failing one address costs exactly two
    // peer ids.
    let mut ledger = ledger();

    for seed in 1..=64 {
        ledger.record(server(seed), endpoint(PUBLIC), ProbeResult::Failed);
    }

    assert_eq!(ledger.failing_addresses(), 1);
    assert_eq!(
        ledger.servers_blaming(&endpoint(PUBLIC)),
        CORROBORATION_THRESHOLD
    );
}

#[test]
fn the_ledger_reaches_the_same_verdicts_every_time_it_is_replayed() {
    // Purity: no clock, no randomness, and — the one a `HashMap` could quietly
    // break — no dependence on iteration order. Each replay builds a fresh
    // ledger, whose hasher is seeded differently, so an order-dependent verdict
    // would diverge here.
    let script: [(u8, &str, ProbeResult); 12] = [
        (1, PUBLIC, ProbeResult::Failed),
        (1, PUBLIC, ProbeResult::Failed),
        (2, OTHER, ProbeResult::Failed),
        (3, OTHER, ProbeResult::Failed),
        (2, PUBLIC, ProbeResult::Failed),
        (4, PUBLIC, ProbeResult::Succeeded),
        (4, PUBLIC, ProbeResult::Succeeded),
        (5, OTHER, ProbeResult::Failed),
        (6, OTHER, ProbeResult::Failed),
        (7, PUBLIC, ProbeResult::Failed),
        (8, PUBLIC, ProbeResult::Failed),
        (9, OTHER, ProbeResult::Succeeded),
    ];

    let replay = || {
        let mut ledger = ledger();
        script
            .iter()
            .map(|(seed, text, result)| ledger.record(server(*seed), endpoint(text), *result))
            .collect::<Vec<_>>()
    };

    let first = replay();
    for _ in 0..8 {
        assert_eq!(replay(), first);
    }
}
