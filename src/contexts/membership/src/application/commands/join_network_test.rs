use std::num::NonZeroUsize;
use std::sync::Arc;

use shared_types::{PeerConnected, PeerId, ProtocolVersion};

use crate::application::commands::{JoinNetwork, JoinNetworkHandler};
use crate::application::{MembershipSettings, MembershipState};
use crate::domain::events::{MembershipEvent, NetworkJoined};
use crate::domain::{DurationMillis, Endpoint, JoinTicket, JoinTicketError, Millis, NetworkStatus};
use crate::ports::port_fakes::{
    FailingPublisher, InMemoryPeerCache, ManualClock, RecordingPublisher, ScriptedDiscovery,
    ScriptedTransport, StatusProbe, UnavailableDiscovery, UnusablePeerCache, UnusableTransport,
};
use crate::ports::{
    BootstrapRung, CachedPeer, ClockPort, DiscoveredPeer, EventPublisherError, EventPublisherPort,
    PeerCacheError, PeerCachePort, PeerDiscoveryError, PeerDiscoveryPort, PeerTransportError,
    PeerTransportPort, RungFailure,
};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);
const LISTEN: &str = "/ip4/0.0.0.0/udp/4001/quic-v1";
const BOB_ADDRESS: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";
const CAROL_ADDRESS: &str = "/ip4/203.0.113.9/udp/4001/quic-v1";
const DAVE_ADDRESS: &str = "/ip4/192.0.2.11/udp/4001/quic-v1";

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn cached(peer: PeerId, address: &str) -> CachedPeer {
    CachedPeer {
        peer,
        endpoints: vec![endpoint(address)],
        last_seen_at: T0,
    }
}

fn discovered(peer: PeerId, address: &str) -> DiscoveredPeer {
    DiscoveredPeer {
        peer,
        endpoints: vec![endpoint(address)],
    }
}

fn ticket_for(peer: PeerId, address: &str, expires_at: Millis) -> JoinTicket {
    JoinTicket::new(
        peer,
        vec![endpoint(address)],
        ProtocolVersion::CURRENT,
        expires_at,
    )
    .expect("a ticket with an endpoint is well formed")
}

/// Everything a join needs, with each port replaceable per test.
struct Ladder {
    state: Arc<MembershipState>,
    clock: Arc<ManualClock>,
    transport: Arc<dyn PeerTransportPort + Send + Sync>,
    discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
    cache: Arc<dyn PeerCachePort + Send + Sync>,
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
}

impl Ladder {
    /// A transport that answers at every listed address, no cached peers, a
    /// silent LAN, and no ticket.
    fn reaching(addresses: &[&str]) -> Self {
        let mut transport = ScriptedTransport::listening_on(vec![endpoint(LISTEN)]);
        for address in addresses {
            transport = transport.reachable_at(endpoint(address));
        }

        Self {
            state: Arc::new(MembershipState::for_local_peer(test_peers::alice())),
            clock: Arc::new(ManualClock::starting_at(T0)),
            transport: Arc::new(transport),
            discovery: Arc::new(ScriptedDiscovery::observing(Vec::new())),
            cache: Arc::new(InMemoryPeerCache::empty()),
            publisher: Arc::new(RecordingPublisher::new()),
        }
    }

    fn handler(&self) -> JoinNetworkHandler {
        JoinNetworkHandler::new(
            MembershipSettings::for_local_peer(test_peers::alice()),
            Arc::clone(&self.state),
            Arc::clone(&self.clock) as Arc<dyn ClockPort + Send + Sync>,
            Arc::clone(&self.transport),
            Arc::clone(&self.discovery),
            Arc::clone(&self.cache),
            Arc::clone(&self.publisher),
        )
    }
}

fn join(ladder: &Ladder, ticket: Option<JoinTicket>) -> crate::ports::JoinOutcome {
    ladder
        .handler()
        .handle(JoinNetwork { ticket })
        .expect("a join that connects nobody is still Ok — Isolated is a state")
}

// ------------------------------------------------------- rung (a): the cache

#[test]
fn a_warm_cache_connects_and_the_costlier_rungs_are_never_reached() {
    let mut ladder = Ladder::reaching(&[BOB_ADDRESS]);
    ladder.cache = Arc::new(InMemoryPeerCache::holding(vec![cached(
        test_peers::bob(),
        BOB_ADDRESS,
    )]));
    let discovery = Arc::new(
        ScriptedDiscovery::observing(vec![discovered(test_peers::carol(), CAROL_ADDRESS)])
            .with_redeemable(discovered(test_peers::dave(), DAVE_ADDRESS)),
    );
    ladder.discovery = Arc::clone(&discovery) as Arc<dyn PeerDiscoveryPort + Send + Sync>;

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, Millis::MAX)),
    );

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        vec![BootstrapRung::CachedPeers],
        "after one successful join a machine never needs another rung (D1)"
    );
    assert_eq!(outcome.diagnostic.connected_peer(), Some(test_peers::bob()));
    assert_eq!(
        discovery.redemptions(),
        0,
        "the ticket was never needed, so it was never spent"
    );
    assert_eq!(
        outcome.status,
        NetworkStatus::Connected(NonZeroUsize::new(1).expect("one peer"))
    );
}

#[test]
fn a_join_announces_the_endpoints_the_transport_is_listening_on() {
    let mut ladder = Ladder::reaching(&[BOB_ADDRESS]);
    let discovery = Arc::new(ScriptedDiscovery::observing(Vec::new()));
    ladder.discovery = Arc::clone(&discovery) as Arc<dyn PeerDiscoveryPort + Send + Sync>;

    join(&ladder, None);

    assert_eq!(
        discovery.announcements(),
        vec![vec![endpoint(LISTEN)]],
        "every instance offers discovery to others (AC4), and joining is public (S8)"
    );
}

// ------------------------------------------------ rung (b): the local network

#[test]
fn an_empty_cache_falls_through_to_the_local_network() {
    let mut ladder = Ladder::reaching(&[CAROL_ADDRESS]);
    ladder.discovery = Arc::new(ScriptedDiscovery::observing(vec![discovered(
        test_peers::carol(),
        CAROL_ADDRESS,
    )]));

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        vec![BootstrapRung::CachedPeers, BootstrapRung::LocalNetwork]
    );
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::NoCandidates),
        "a fresh install has an empty cache; that is the case the ladder exists for"
    );
    assert_eq!(
        outcome.diagnostic.connected_peer(),
        Some(test_peers::carol())
    );
}

#[test]
fn a_cache_that_cannot_be_read_costs_a_rung_and_nothing_else() {
    let mut ladder = Ladder::reaching(&[CAROL_ADDRESS]);
    ladder.cache = Arc::new(UnusablePeerCache(
        PeerCacheError::UnsupportedSchemaVersion { found: 9 },
    ));
    ladder.discovery = Arc::new(ScriptedDiscovery::observing(vec![discovered(
        test_peers::carol(),
        CAROL_ADDRESS,
    )]));

    let outcome = join(&ladder, None);

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::Cache(
            PeerCacheError::UnsupportedSchemaVersion { found: 9 }
        )),
        "S4: a foreign cache is reported, never rewritten, and never fatal"
    );
}

#[test]
fn the_local_peers_own_announcement_is_not_a_bootstrap_candidate() {
    let mut ladder = Ladder::reaching(&[CAROL_ADDRESS]);
    ladder.discovery = Arc::new(ScriptedDiscovery::observing(vec![
        discovered(test_peers::alice(), LISTEN),
        discovered(test_peers::carol(), CAROL_ADDRESS),
    ]));

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.connected_peer(),
        Some(test_peers::carol())
    );
    assert!(
        ladder
            .state
            .read(|roster| roster.peer(&test_peers::alice()).is_none()),
        "invariant 2, on the path where hearing yourself is routine"
    );
}

// -------------------------------------------------- rung (c): the join ticket

#[test]
fn a_quiet_lan_falls_through_to_the_pasted_ticket() {
    let mut ladder = Ladder::reaching(&[DAVE_ADDRESS]);
    let discovery = Arc::new(
        ScriptedDiscovery::observing(Vec::new())
            .with_redeemable(discovered(test_peers::dave(), DAVE_ADDRESS)),
    );
    ladder.discovery = Arc::clone(&discovery) as Arc<dyn PeerDiscoveryPort + Send + Sync>;

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, Millis::MAX)),
    );

    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        BootstrapRung::LADDER.to_vec()
    );
    assert_eq!(
        outcome.diagnostic.connected_peer(),
        Some(test_peers::dave()),
        "the honest cost of serverless internet reach: one pasted ticket, once (D1)"
    );
    assert_eq!(discovery.redemptions(), 1);
}

#[test]
fn a_launch_with_no_ticket_reaches_the_last_rung_and_reports_it_had_nothing_to_try() {
    let ladder = Ladder::reaching(&[]);

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::NoCandidates)
    );
    assert_eq!(outcome.status, NetworkStatus::Isolated);
}

#[test]
fn an_expired_ticket_is_rejected_with_its_own_typed_error_and_never_reaches_the_network() {
    let mut ladder = Ladder::reaching(&[DAVE_ADDRESS]);
    let discovery = Arc::new(
        ScriptedDiscovery::observing(Vec::new())
            .with_redeemable(discovered(test_peers::dave(), DAVE_ADDRESS)),
    );
    ladder.discovery = Arc::clone(&discovery) as Arc<dyn PeerDiscoveryPort + Send + Sync>;
    let expires_at = T0.saturating_add(DurationMillis::from_secs(10));
    ladder.clock.advance(DurationMillis::from_secs(60));

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, expires_at)),
    );

    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::Ticket(JoinTicketError::Expired {
            expires_at,
            now: T0.saturating_add(DurationMillis::from_secs(60)),
        })),
        "'expired' is the actionable answer: ask the issuer for a fresh one"
    );
    assert_eq!(
        discovery.redemptions(),
        0,
        "an unusable ticket is never handed to the adapter"
    );
    assert_eq!(outcome.status, NetworkStatus::Isolated);
}

#[test]
fn a_ticket_from_a_different_protocol_major_is_rejected() {
    let mut ladder = Ladder::reaching(&[DAVE_ADDRESS]);
    let discovery = Arc::new(
        ScriptedDiscovery::observing(Vec::new())
            .with_redeemable(discovered(test_peers::dave(), DAVE_ADDRESS)),
    );
    ladder.discovery = Arc::clone(&discovery) as Arc<dyn PeerDiscoveryPort + Send + Sync>;
    let foreign = ProtocolVersion::new(ProtocolVersion::CURRENT.major + 1, 0);
    let ticket = JoinTicket::new(
        test_peers::dave(),
        vec![endpoint(DAVE_ADDRESS)],
        foreign,
        Millis::MAX,
    )
    .expect("well formed");

    let outcome = join(&ladder, Some(ticket));

    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::Ticket(JoinTicketError::IncompatibleProtocol {
            ticket: foreign,
            supported: ProtocolVersion::CURRENT,
        })),
        "S2/AC14: a different major is rejected with a logged reason"
    );
    assert_eq!(discovery.redemptions(), 0);
}

#[test]
fn a_newer_minor_version_on_a_ticket_is_tolerated() {
    let mut ladder = Ladder::reaching(&[DAVE_ADDRESS]);
    ladder.discovery = Arc::new(
        ScriptedDiscovery::observing(Vec::new())
            .with_redeemable(discovered(test_peers::dave(), DAVE_ADDRESS)),
    );
    let newer = ProtocolVersion::new(
        ProtocolVersion::CURRENT.major,
        ProtocolVersion::CURRENT.minor + 1,
    );
    let ticket = JoinTicket::new(
        test_peers::dave(),
        vec![endpoint(DAVE_ADDRESS)],
        newer,
        Millis::MAX,
    )
    .expect("well formed");

    let outcome = join(&ladder, Some(ticket));

    assert!(
        outcome.succeeded(),
        "peers upgrade independently; an additive minor must stay readable (S2)"
    );
}

#[test]
fn a_ticket_nobody_answers_is_reported_as_the_adapters_own_reason() {
    let mut ladder = Ladder::reaching(&[]);
    // No redeemable peer scripted: the endpoints in the ticket do not answer.
    ladder.discovery = Arc::new(ScriptedDiscovery::observing(Vec::new()));

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, Millis::MAX)),
    );

    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::Discovery(
            PeerDiscoveryError::TicketUnreachable
        )),
        "the issuer may be offline or have moved — a diagnostic, never a hang (AC3)"
    );
}

// ------------------------------------------------------- every rung fails

#[test]
fn when_every_rung_fails_the_peer_is_isolated_with_a_diagnostic_naming_all_of_them() {
    // AC3's exact shape. Each rung fails for its own reason, and each reason
    // is a different sentence to the user.
    let mut ladder = Ladder::reaching(&[]);
    ladder.cache = Arc::new(UnusablePeerCache(PeerCacheError::Unreadable));
    ladder.discovery = Arc::new(
        ScriptedDiscovery::observing(Vec::new())
            .with_observation_failure(PeerDiscoveryError::Unavailable),
    );
    let expires_at = T0.saturating_add(DurationMillis::from_secs(10));
    ladder.clock.advance(DurationMillis::from_secs(60));

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, expires_at)),
    );

    assert!(!outcome.succeeded());
    assert_eq!(outcome.joined, None);
    assert_eq!(
        outcome.status,
        NetworkStatus::Isolated,
        "Isolated is a normal state, not an error"
    );
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        BootstrapRung::LADDER.to_vec()
    );
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::Cache(PeerCacheError::Unreadable))
    );
    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::LocalNetwork),
        Some(RungFailure::Discovery(PeerDiscoveryError::Unavailable))
    );
    assert!(matches!(
        outcome.diagnostic.failure_of(BootstrapRung::JoinTicket),
        Some(RungFailure::Ticket(JoinTicketError::Expired { .. }))
    ));

    let rendered = outcome.diagnostic.to_string();
    for rung in BootstrapRung::LADDER {
        assert!(
            rendered.contains(&rung.to_string()),
            "AC3: the diagnostic must be visible and name what was tried:\n{rendered}"
        );
    }
}

#[test]
fn peers_that_were_found_but_did_not_answer_are_counted_in_the_diagnostic() {
    let mut ladder = Ladder::reaching(&[]);
    ladder.cache = Arc::new(InMemoryPeerCache::holding(vec![
        cached(test_peers::bob(), BOB_ADDRESS),
        cached(test_peers::carol(), CAROL_ADDRESS),
    ]));

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.failure_of(BootstrapRung::CachedPeers),
        Some(RungFailure::Unreachable { candidates: 2 }),
        "S7's known limit: with no reachable peer, two symmetric-NAT peers cannot connect"
    );
    assert!(
        ladder.state.read(|roster| roster.len()) == 2,
        "the peers stay known even though none answered; the next launch tries again"
    );
}

#[test]
fn a_transport_that_cannot_listen_still_walks_the_ladder_and_says_it_is_unreachable() {
    let mut ladder = Ladder::reaching(&[]);
    ladder.transport = Arc::new(UnusableTransport(PeerTransportError::ListenFailed));

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.listen_failure,
        Some(PeerTransportError::ListenFailed),
        "dialling out still works; nobody dialling back is the part a user cannot otherwise see"
    );
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        BootstrapRung::LADDER.to_vec()
    );
}

#[test]
fn a_discovery_mechanism_that_rejects_the_announcement_is_reported() {
    let mut ladder = Ladder::reaching(&[]);
    ladder.discovery = Arc::new(UnavailableDiscovery);

    let outcome = join(&ladder, None);

    assert_eq!(
        outcome.diagnostic.announce_failure,
        Some(PeerDiscoveryError::Unavailable)
    );
}

// ---------------------------------------------------- status and events

#[test]
fn the_status_reads_joining_at_every_rung_and_never_after() {
    // The ladder is synchronous, so the only vantage point inside it is a port
    // it calls. Both rungs that touch a port sample the status from there.
    let mut ladder = Ladder::reaching(&[]);
    let probe = Arc::new(StatusProbe::watching({
        let state = Arc::clone(&ladder.state);
        move || state.network_status()
    }));
    ladder.cache = Arc::new(InMemoryPeerCache::empty().with_status_probe(Arc::clone(&probe)));
    ladder.discovery =
        Arc::new(ScriptedDiscovery::observing(Vec::new()).with_status_probe(Arc::clone(&probe)));

    let outcome = join(
        &ladder,
        Some(ticket_for(test_peers::dave(), DAVE_ADDRESS, Millis::MAX)),
    );

    assert_eq!(
        probe.observed(),
        vec![
            NetworkStatus::Joining,
            NetworkStatus::Joining,
            NetworkStatus::Joining
        ],
        "the cache load, the LAN observation, and the ticket redemption all happen mid-join"
    );
    assert_eq!(outcome.status, NetworkStatus::Isolated);
    assert_eq!(ladder.state.network_status(), NetworkStatus::Isolated);
}

#[test]
fn the_joining_phase_ends_even_when_the_join_gives_up_mid_ladder() {
    let mut ladder = Ladder::reaching(&[BOB_ADDRESS]);
    ladder.cache = Arc::new(InMemoryPeerCache::holding(vec![cached(
        test_peers::bob(),
        BOB_ADDRESS,
    )]));
    ladder.publisher = Arc::new(FailingPublisher(EventPublisherError::Unavailable));

    let outcome = ladder.handler().handle(JoinNetwork { ticket: None });

    assert_eq!(outcome, Err(EventPublisherError::Unavailable));
    assert_ne!(
        ladder.state.network_status(),
        NetworkStatus::Joining,
        "a status latched on Joining is indistinguishable from the hang AC3 forbids"
    );
}

#[test]
fn a_join_publishes_the_discovery_the_connection_and_then_the_arrival() {
    let mut ladder = Ladder::reaching(&[BOB_ADDRESS]);
    ladder.cache = Arc::new(InMemoryPeerCache::holding(vec![cached(
        test_peers::bob(),
        BOB_ADDRESS,
    )]));
    let publisher = Arc::new(RecordingPublisher::new());
    ladder.publisher = Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>;

    let outcome = join(&ladder, None);

    let joined = NetworkJoined {
        at: T0,
        connected_peers: NonZeroUsize::new(1).expect("one peer"),
    };
    assert_eq!(outcome.joined, Some(joined));
    assert_eq!(
        publisher.published(),
        vec![
            MembershipEvent::PeerDiscovered(crate::domain::events::PeerDiscovered {
                peer: test_peers::bob(),
                at: T0,
            }),
            MembershipEvent::PeerConnected(PeerConnected {
                peer: test_peers::bob()
            }),
            MembershipEvent::NetworkJoined(joined),
        ],
        "a consumer must never see the network joined before the session that joined it"
    );
}

#[test]
fn a_join_that_connects_nobody_announces_nothing() {
    let ladder = Ladder::reaching(&[]);
    let publisher = Arc::new(RecordingPublisher::new());
    let mut ladder = Ladder {
        publisher: Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        ..ladder
    };
    ladder.cache = Arc::new(InMemoryPeerCache::empty());

    let outcome = join(&ladder, None);

    assert_eq!(outcome.joined, None);
    assert_eq!(
        publisher.published(),
        Vec::new(),
        "isolation is a state; a state is not an event"
    );
}

#[test]
fn a_join_over_an_existing_connection_that_adds_nothing_announces_no_second_arrival() {
    let mut ladder = Ladder::reaching(&[BOB_ADDRESS]);
    ladder.cache = Arc::new(InMemoryPeerCache::holding(vec![cached(
        test_peers::bob(),
        BOB_ADDRESS,
    )]));
    let publisher = Arc::new(RecordingPublisher::new());
    ladder.publisher = Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>;
    let handler = ladder.handler();

    handler
        .handle(JoinNetwork { ticket: None })
        .expect("first join");
    let published_after_first = publisher.published().len();
    let second = handler
        .handle(JoinNetwork { ticket: None })
        .expect("a second join is harmless");

    assert_eq!(
        second.joined, None,
        "the ladder connected nobody new, so nothing arrived"
    );
    assert!(second.status.connected_peers() == 1);
    assert_eq!(publisher.published().len(), published_after_first);
}
