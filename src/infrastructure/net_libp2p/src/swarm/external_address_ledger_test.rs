use libp2p::identity::Keypair;
use libp2p::{Multiaddr, PeerId as Libp2pPeerId};

use crate::limits::ResourceLimits;
use crate::swarm::external_address_ledger::{
    CORROBORATION_THRESHOLD, CandidateRejection, ExternalAddressLedger, Promotion,
};

/// A globally routable address. TEST-NET-3 (RFC 5737) rather than a real
/// public address: it is reserved for documentation, so nothing in a test can
/// ever be mistaken for somebody's machine.
const PUBLIC: &str = "/ip4/203.0.113.7/tcp/4001";

/// Every shape D5 refuses, with the reason it is refused.
///
/// This is the table P1-5 is asserted against, and it is deliberately written
/// out rather than generated: each row is a class of address a peer on the same
/// LAN, behind the same carrier NAT, or on the same host would observe us at,
/// and advertising any of them globally would publish an address a stranger
/// cannot dial.
const NON_GLOBAL: [(&str, &str); 16] = [
    ("/ip4/127.0.0.1/tcp/4001", "IPv4 loopback"),
    ("/ip4/10.0.0.4/tcp/4001", "RFC 1918 private, 10/8"),
    ("/ip4/172.16.3.9/tcp/4001", "RFC 1918 private, 172.16/12"),
    ("/ip4/192.168.1.20/tcp/4001", "RFC 1918 private, 192.168/16"),
    ("/ip4/169.254.7.7/tcp/4001", "IPv4 link-local"),
    ("/ip4/100.64.0.1/tcp/4001", "CGNAT, low edge of 100.64/10"),
    (
        "/ip4/100.127.255.254/tcp/4001",
        "CGNAT, high edge of 100.64/10",
    ),
    ("/ip4/0.0.0.0/tcp/4001", "IPv4 unspecified"),
    ("/ip4/224.0.0.1/tcp/4001", "IPv4 multicast"),
    ("/ip4/255.255.255.255/tcp/4001", "IPv4 broadcast"),
    ("/ip6/::1/tcp/4001", "IPv6 loopback"),
    ("/ip6/::/tcp/4001", "IPv6 unspecified"),
    ("/ip6/fd00::1/tcp/4001", "IPv6 unique local, fc00::/7"),
    ("/ip6/fe80::1/tcp/4001", "IPv6 link-local, fe80::/10"),
    (
        "/ip6/::ffff:192.168.0.4/tcp/4001",
        "IPv4-mapped IPv6 carrying a private address",
    ),
    (
        "/dns4/example.com/tcp/4001",
        "no IP literal at all, so nothing can be judged global",
    ),
];

fn address(text: &str) -> Multiaddr {
    text.parse().expect("a well-formed multiaddress")
}

/// A distinct, deterministic observer per seed.
///
/// Derived from a keypair rather than randomised so that a failing run can be
/// re-run and say the same thing (AC13).
fn observer(seed: u8) -> Libp2pPeerId {
    Keypair::ed25519_from_bytes([seed; 32])
        .expect("32 bytes are a valid Ed25519 seed")
        .public()
        .to_peer_id()
}

/// This peer's own identity, which is never a valid observer of itself.
fn local() -> Libp2pPeerId {
    observer(0)
}

fn ledger() -> ExternalAddressLedger {
    let limits = ResourceLimits::DEFAULT;
    ExternalAddressLedger::new(
        local(),
        limits.max_candidate_addresses,
        limits.max_observers_per_address,
    )
}

/// A relay address: reachable *through* another peer, which is not the thing
/// this piece establishes (D5).
fn circuit() -> Multiaddr {
    address(&format!("{PUBLIC}/p2p/{}/p2p-circuit", observer(9)))
}

#[test]
fn a_first_observation_is_recorded_and_never_promoted() {
    // P1-1: one peer's word is a candidate, not an advertisement.
    let mut ledger = ledger();

    assert_eq!(
        ledger.observe(observer(1), address(PUBLIC)),
        Promotion::Recorded { corroborations: 1 }
    );
    assert!(!ledger.is_promoted(&address(PUBLIC)));
    assert_eq!(ledger.candidate_count(), 1);
}

#[test]
fn a_second_distinct_observer_promotes_the_address_exactly_once() {
    // P1-2, and the whole point of the threshold: two peers who have never met
    // reporting the same address is the smallest rule that is not "trust
    // anyone".
    let mut ledger = ledger();
    ledger.observe(observer(1), address(PUBLIC));

    assert_eq!(
        ledger.observe(observer(2), address(PUBLIC)),
        Promotion::Promote(address(PUBLIC))
    );
    assert!(ledger.is_promoted(&address(PUBLIC)));
    assert_eq!(
        ledger.candidate_count(),
        0,
        "a promoted address is no longer awaiting corroboration"
    );

    assert_eq!(
        ledger.observe(observer(3), address(PUBLIC)),
        Promotion::Ignored(CandidateRejection::AlreadyPromoted),
        "a third observer does not promote what is already promoted"
    );
}

#[test]
fn one_peer_repeating_itself_never_reaches_the_threshold() {
    // S2: corroboration counts *distinct* observers. If it counted
    // observations, a single hostile peer would meet any threshold alone by
    // saying the same thing twice — identities being free, that is the whole
    // attack.
    let mut ledger = ledger();

    for _ in 0..64 {
        assert_eq!(
            ledger.observe(observer(1), address(PUBLIC)),
            Promotion::Recorded { corroborations: 1 }
        );
    }

    assert!(!ledger.is_promoted(&address(PUBLIC)));
}

#[test]
fn a_promoted_address_observed_again_is_refused_rather_than_promoted_twice() {
    // Invariant 1. Re-promotion would re-enter the confirmation path and make
    // the composition root re-announce for no new information.
    let mut ledger = ledger();
    ledger.observe(observer(1), address(PUBLIC));
    ledger.observe(observer(2), address(PUBLIC));

    for seed in 1..=6 {
        assert_eq!(
            ledger.observe(observer(seed), address(PUBLIC)),
            Promotion::Ignored(CandidateRejection::AlreadyPromoted)
        );
    }
}

#[test]
fn no_non_global_address_is_recorded_however_many_peers_report_it() {
    // P1-5 and S3. Two peers on one LAN both observe each other at
    // `192.168.x.x`, which trivially meets the threshold — so the filter has to
    // sit *before* the counting, not beside it.
    for (text, why) in NON_GLOBAL {
        let mut ledger = ledger();

        for seed in 1..=16 {
            assert_eq!(
                ledger.observe(observer(seed), address(text)),
                Promotion::Ignored(CandidateRejection::NotGlobal),
                "{text} ({why}) must never be recorded"
            );
        }

        assert_eq!(ledger.candidate_count(), 0, "{text} ({why})");
        assert!(!ledger.is_promoted(&address(text)), "{text} ({why})");
    }
}

#[test]
fn a_relay_circuit_address_is_never_promoted_however_public_its_relay_is() {
    // The relay's own address is global; the circuit through it is not a
    // directly dialable address of ours, and relay reachability is not what
    // this piece establishes (D5).
    let mut ledger = ledger();

    for seed in 1..=8 {
        assert_eq!(
            ledger.observe(observer(seed), circuit()),
            Promotion::Ignored(CandidateRejection::NotGlobal)
        );
    }

    assert_eq!(ledger.candidate_count(), 0);
}

#[test]
fn an_address_this_peer_reports_about_itself_is_refused() {
    // A peer corroborating its own address would be one observer counted as
    // two the moment any other peer agreed.
    let mut ledger = ledger();

    assert_eq!(
        ledger.observe(local(), address(PUBLIC)),
        Promotion::Ignored(CandidateRejection::SelfObservation)
    );
    assert_eq!(ledger.candidate_count(), 0);
}

#[test]
fn the_candidate_address_bound_holds_under_a_flood() {
    // P1-6/S5: candidate addresses arrive from untrusted peers, and a peer that
    // reports a fresh address per identify exchange must not be able to grow
    // this process without limit.
    let cap = ResourceLimits::DEFAULT.max_candidate_addresses;
    let mut ledger = ledger();
    let flood = cap * 4;
    let mut refused = 0;

    for index in 0..flood {
        let address = address(&format!("/ip4/203.0.113.7/tcp/{}", 4000 + index));
        match ledger.observe(observer(1), address) {
            Promotion::Recorded { corroborations } => assert_eq!(corroborations, 1),
            Promotion::Ignored(CandidateRejection::LedgerFull) => refused += 1,
            other => panic!("unexpected verdict {other:?}"),
        }
    }

    assert_eq!(ledger.candidate_count(), cap);
    assert_eq!(refused, flood - cap);

    // A full ledger still corroborates what it already holds: the bound refuses
    // *new* addresses, it does not stop a candidate from being confirmed.
    let held = address("/ip4/203.0.113.7/tcp/4000");
    assert_eq!(
        ledger.observe(observer(2), held.clone()),
        Promotion::Promote(held)
    );
}

#[test]
fn a_promoted_address_still_occupies_a_slot_in_the_bound() {
    // Promotion moves an address from "awaiting corroboration" to "confirmed";
    // it does not free the memory it occupies, and a bound that ignored the
    // promoted set would be a bound on half the ledger.
    let cap = 4;
    let mut ledger = ExternalAddressLedger::new(local(), cap, 8);

    let promoted = address("/ip4/203.0.113.7/tcp/4001");
    ledger.observe(observer(1), promoted.clone());
    assert_eq!(
        ledger.observe(observer(2), promoted),
        Promotion::Promote(address("/ip4/203.0.113.7/tcp/4001"))
    );

    for index in 1..cap {
        assert_eq!(
            ledger.observe(
                observer(1),
                address(&format!("/ip4/203.0.113.7/tcp/{}", 5000 + index))
            ),
            Promotion::Recorded { corroborations: 1 }
        );
    }

    assert_eq!(
        ledger.observe(observer(1), address("/ip4/203.0.113.7/tcp/6000")),
        Promotion::Ignored(CandidateRejection::LedgerFull)
    );
}

#[test]
fn the_per_address_observer_bound_holds_under_a_flood_of_identities() {
    // The other half of P1-6: one address reported by an endless crowd. At the
    // shipped threshold of two the address promotes before this cap could ever
    // bind, so the cap is exercised with a threshold raised above it — which is
    // the only way to prove the bound is enforced rather than merely written
    // down.
    let cap = 4;
    let mut ledger = ExternalAddressLedger::with_threshold(local(), cap + 1, 8, cap);
    let flood = 32;
    let mut refused = 0;

    for seed in 1..=flood {
        match ledger.observe(observer(seed), address(PUBLIC)) {
            Promotion::Recorded { corroborations } => assert!(corroborations <= cap),
            Promotion::Ignored(CandidateRejection::LedgerFull) => refused += 1,
            other => panic!("unexpected verdict {other:?}"),
        }
    }

    assert_eq!(refused, usize::from(flood) - cap);
    assert_eq!(ledger.corroborations(&address(PUBLIC)), cap);

    // A peer already counted is not refused when the set is full: it is not
    // asking for a new slot.
    assert_eq!(
        ledger.observe(observer(1), address(PUBLIC)),
        Promotion::Recorded {
            corroborations: cap
        }
    );
}

#[test]
fn the_ledger_reaches_the_same_verdicts_every_time_it_is_replayed() {
    // Invariant 5: a pure decision over its own state and the observation. No
    // clock, no randomness, and — the one that a `HashMap` could quietly break
    // — no dependence on iteration order. Each replay builds a fresh ledger,
    // whose hasher is seeded differently, so an order-dependent verdict would
    // diverge here.
    let script: [(u8, &str); 12] = [
        (1, PUBLIC),
        (1, PUBLIC),
        (2, "/ip4/192.168.1.20/tcp/4001"),
        (3, "/ip4/203.0.113.8/tcp/4001"),
        (0, PUBLIC),
        (2, PUBLIC),
        (4, PUBLIC),
        (4, "/ip4/203.0.113.8/tcp/4001"),
        (5, "/ip6/2001:db8::1/tcp/4001"),
        (6, "/ip6/fe80::1/tcp/4001"),
        (5, "/ip6/2001:db8::1/tcp/4001"),
        (7, "/ip6/2001:db8::1/tcp/4001"),
    ];

    let replay = || {
        let mut ledger = ledger();
        script
            .iter()
            .map(|(seed, text)| ledger.observe(observer(*seed), address(text)))
            .collect::<Vec<_>>()
    };

    let first = replay();
    for _ in 0..8 {
        assert_eq!(replay(), first);
    }
}

#[test]
fn the_shipped_threshold_is_two_and_is_not_one() {
    // S2 written as an assertion. Lowering this to 1 reintroduces the
    // misdirection vector the whole piece exists to close, and a change to it
    // needs `$spdd-prompt-update` rather than an edit to this constant.
    assert_eq!(CORROBORATION_THRESHOLD, 2);
}
