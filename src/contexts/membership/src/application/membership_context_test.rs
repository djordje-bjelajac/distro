use std::num::NonZeroUsize;
use std::sync::Arc;

use shared_types::{PeerConnected, PeerDisconnected};

use crate::application::{MembershipContext, MembershipSettings};
use crate::domain::events::MembershipEvent;
use crate::domain::{
    DurationMillis, Endpoint, LivenessWindows, Millis, NetworkStatus, Presence, SessionState,
};
use crate::ports::port_fakes::{
    InMemoryPeerCache, ManualClock, RecordingPublisher, ScriptedDiscovery, ScriptedTransport,
};
use crate::ports::{
    CachedPeer, ClockPort, DiscoveredPeer, EventPublisherPort, InboundSessionPort, JoinNetworkPort,
    MembershipQueryPort, PeerCachePort, PeerDiscoveryPort, PeerTransportPort,
};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);
const LISTEN: &str = "/ip4/0.0.0.0/udp/4001/quic-v1";
const BOB_ADDRESS: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";
const CAROL_ADDRESS: &str = "/ip4/203.0.113.9/udp/4001/quic-v1";

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

struct Wiring {
    clock: Arc<ManualClock>,
    cache: Arc<InMemoryPeerCache>,
    publisher: Arc<RecordingPublisher>,
    context: MembershipContext,
}

/// A context wired the way a composition root would wire it, with a transport
/// that answers at `BOB_ADDRESS` and a LAN holding nobody.
fn wiring_with_cache(cached: Vec<CachedPeer>) -> Wiring {
    let clock = Arc::new(ManualClock::starting_at(T0));
    let cache = Arc::new(InMemoryPeerCache::holding(cached));
    let publisher = Arc::new(RecordingPublisher::new());
    let transport = Arc::new(
        ScriptedTransport::listening_on(vec![endpoint(LISTEN)]).reachable_at(endpoint(BOB_ADDRESS)),
    );
    let discovery = Arc::new(ScriptedDiscovery::observing(Vec::new()));

    let context = MembershipContext::new(
        MembershipSettings::for_local_peer(test_peers::alice()),
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        transport as Arc<dyn PeerTransportPort + Send + Sync>,
        discovery as Arc<dyn PeerDiscoveryPort + Send + Sync>,
        Arc::clone(&cache) as Arc<dyn PeerCachePort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    Wiring {
        clock,
        cache,
        publisher,
        context,
    }
}

fn cached_bob() -> CachedPeer {
    CachedPeer {
        peer: test_peers::bob(),
        endpoints: vec![endpoint(BOB_ADDRESS)],
        last_seen_at: T0,
    }
}

#[test]
fn the_queries_see_what_the_join_did() {
    let w = wiring_with_cache(vec![cached_bob()]);
    let join: &dyn JoinNetworkPort = w.context.join();
    let queries: &dyn MembershipQueryPort = w.context.queries();

    let outcome = join.join_network(None).expect("join");

    assert!(outcome.succeeded());
    assert_eq!(
        queries.network_status(),
        NetworkStatus::Connected(NonZeroUsize::new(1).expect("one peer"))
    );
    let peers = queries.known_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].peer, test_peers::bob());
    assert!(peers[0].is_connected());
    assert_eq!(queries.online_peers(), vec![test_peers::bob()]);
}

#[test]
fn the_command_and_query_sides_share_one_roster() {
    let w = wiring_with_cache(Vec::new());
    let sessions: &dyn InboundSessionPort = w.context.sessions();
    let queries: &dyn MembershipQueryPort = w.context.queries();

    sessions
        .peer_observed(DiscoveredPeer {
            peer: test_peers::carol(),
            endpoints: vec![endpoint(CAROL_ADDRESS)],
        })
        .expect("observation");

    assert_eq!(
        queries
            .known_peers()
            .iter()
            .map(|view| view.peer)
            .collect::<Vec<_>>(),
        vec![test_peers::carol()],
        "two divergent rosters would be a defect visible only at runtime, in the UI"
    );
}

#[test]
fn an_inbound_session_runs_the_full_lifecycle_through_one_port() {
    let w = wiring_with_cache(Vec::new());
    let sessions: &dyn InboundSessionPort = w.context.sessions();
    let queries: &dyn MembershipQueryPort = w.context.queries();

    sessions
        .session_opened(test_peers::carol(), vec![endpoint(CAROL_ADDRESS)])
        .expect("a stranger that redeemed our ticket dials in");
    assert_eq!(queries.network_status(), NetworkStatus::Isolated);

    sessions
        .session_established(test_peers::carol())
        .expect("handshake");
    assert_eq!(
        queries.network_status(),
        NetworkStatus::Connected(NonZeroUsize::new(1).expect("one peer"))
    );

    sessions
        .session_closed(test_peers::carol())
        .expect("the link dropped");
    assert_eq!(queries.network_status(), NetworkStatus::Isolated);

    assert_eq!(
        w.publisher.cross_context(),
        vec![
            MembershipEvent::PeerConnected(PeerConnected {
                peer: test_peers::carol()
            }),
            MembershipEvent::PeerDisconnected(PeerDisconnected {
                peer: test_peers::carol()
            }),
        ],
        "exactly one connect and one disconnect cross the boundary, in that order"
    );
}

#[test]
fn a_session_that_never_established_crosses_no_boundary_at_all() {
    let w = wiring_with_cache(Vec::new());
    let sessions: &dyn InboundSessionPort = w.context.sessions();

    sessions
        .session_opened(test_peers::carol(), vec![endpoint(CAROL_ADDRESS)])
        .expect("open");
    sessions
        .session_closed(test_peers::carol())
        .expect("the handshake never finished");

    assert_eq!(
        w.publisher.cross_context(),
        Vec::new(),
        "messaging must not fail directs for a peer it never considered reachable (D10)"
    );
}

#[test]
fn presence_expires_through_the_clock_and_the_query_side_agrees() {
    let w = wiring_with_cache(Vec::new());
    let sessions: &dyn InboundSessionPort = w.context.sessions();
    let queries: &dyn MembershipQueryPort = w.context.queries();

    sessions
        .peer_observed(DiscoveredPeer {
            peer: test_peers::carol(),
            endpoints: vec![endpoint(CAROL_ADDRESS)],
        })
        .expect("observation");
    assert_eq!(queries.online_peers(), vec![test_peers::carol()]);

    w.clock.advance(DurationMillis::from_secs(61));
    let expired = sessions.expire_presence().expect("sweep");

    assert_eq!(
        expired.iter().map(|event| event.peer).collect::<Vec<_>>(),
        vec![test_peers::carol()],
        "AC5: peers observe a departure within the liveness window"
    );
    assert_eq!(queries.online_peers(), Vec::new());
    assert_eq!(queries.known_peers()[0].presence, Presence::Offline);
    assert_eq!(
        queries.known_peers()[0].session,
        None,
        "silence is not a closed link — there simply never was one"
    );
}

#[test]
fn a_heartbeat_keeps_a_connected_peer_online() {
    let w = wiring_with_cache(Vec::new());
    let sessions: &dyn InboundSessionPort = w.context.sessions();
    let queries: &dyn MembershipQueryPort = w.context.queries();

    sessions
        .session_opened(test_peers::carol(), vec![endpoint(CAROL_ADDRESS)])
        .expect("open");
    sessions
        .session_established(test_peers::carol())
        .expect("handshake");

    w.clock.advance(DurationMillis::from_secs(40));
    assert_eq!(queries.known_peers()[0].presence, Presence::Stale);

    sessions
        .peer_heartbeat(test_peers::carol())
        .expect("keep-alive");

    assert_eq!(queries.known_peers()[0].presence, Presence::Online);
    assert_eq!(
        queries.known_peers()[0].session,
        Some(SessionState::Established),
        "a link staying open is not evidence of life; the heartbeat is"
    );
}

#[test]
fn leaving_writes_the_cache_the_next_launch_bootstraps_from() {
    let w = wiring_with_cache(vec![cached_bob()]);
    let join: &dyn JoinNetworkPort = w.context.join();

    join.join_network(None).expect("join");
    let outcome = join.leave_network().expect("leave");

    assert_eq!(outcome.cached_peers, 1);
    assert!(w.cache.saves() >= 1);
    assert_eq!(
        w.cache
            .load()
            .expect("read")
            .iter()
            .map(|entry| entry.peer)
            .collect::<Vec<_>>(),
        vec![test_peers::bob()]
    );
    assert_eq!(
        w.context.queries().network_status(),
        NetworkStatus::Isolated
    );
}

#[test]
fn a_restart_over_the_same_cache_rejoins_without_a_ticket() {
    // The whole point of D1's first rung: the ticket is a one-time cost.
    let first = wiring_with_cache(vec![cached_bob()]);
    first.context.join().join_network(None).expect("first join");
    first.context.join().leave_network().expect("leave");
    let carried_over = first.cache.load().expect("read");

    let second = wiring_with_cache(carried_over);
    let outcome = second
        .context
        .join()
        .join_network(None)
        .expect("later launch");

    assert!(outcome.succeeded());
    assert_eq!(
        outcome.diagnostic.rungs_tried(),
        vec![crate::ports::BootstrapRung::CachedPeers]
    );
}

#[test]
fn connecting_to_a_known_peer_on_purpose_goes_through_the_decision_port() {
    let w = wiring_with_cache(Vec::new());
    let join: &dyn JoinNetworkPort = w.context.join();
    let sessions: &dyn InboundSessionPort = w.context.sessions();

    sessions
        .peer_observed(DiscoveredPeer {
            peer: test_peers::bob(),
            endpoints: vec![endpoint(BOB_ADDRESS)],
        })
        .expect("observation");
    let outcome = join
        .connect_to_peer(test_peers::bob())
        .expect("the endpoint answers");

    assert_eq!(
        outcome.connected,
        Some(PeerConnected {
            peer: test_peers::bob()
        })
    );
    assert_eq!(
        w.context.queries().network_status(),
        NetworkStatus::Connected(NonZeroUsize::new(1).expect("one peer"))
    );
}

#[test]
fn the_query_side_writes_nothing_however_often_it_is_driven() {
    let w = wiring_with_cache(vec![cached_bob()]);
    w.context.join().join_network(None).expect("join");
    let published_after_commands = w.publisher.published().len();
    let saves_after_commands = w.cache.saves();

    for _ in 0..5 {
        w.clock.advance(DurationMillis::from_secs(20));
        let _ = w.context.queries().known_peers();
        let _ = w.context.queries().online_peers();
        let _ = w.context.queries().network_status();
    }

    assert_eq!(w.publisher.published().len(), published_after_commands);
    assert_eq!(w.cache.saves(), saves_after_commands);
    assert_eq!(
        w.context.queries().known_peers()[0].presence,
        Presence::Offline,
        "a peer that aged out is reported as such, and reporting it changed nothing"
    );
}

#[test]
fn custom_liveness_windows_reach_both_the_sweep_and_the_read_model() {
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    let windows = LivenessWindows::new(
        DurationMillis::from_millis(100),
        DurationMillis::from_millis(200),
    )
    .expect("online is shorter than offline");
    let context = MembershipContext::new(
        MembershipSettings::for_local_peer(test_peers::alice()).with_liveness_windows(windows),
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::new(ScriptedTransport::listening_on(Vec::new()))
            as Arc<dyn PeerTransportPort + Send + Sync>,
        Arc::new(ScriptedDiscovery::observing(Vec::new()))
            as Arc<dyn PeerDiscoveryPort + Send + Sync>,
        Arc::new(InMemoryPeerCache::empty()) as Arc<dyn PeerCachePort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    context
        .sessions()
        .peer_observed(DiscoveredPeer {
            peer: test_peers::carol(),
            endpoints: vec![endpoint(CAROL_ADDRESS)],
        })
        .expect("observation");
    clock.advance(DurationMillis::from_millis(200));

    assert_eq!(
        context.sessions().expire_presence().expect("sweep").len(),
        1
    );
    assert_eq!(
        context.queries().known_peers()[0].presence,
        Presence::Offline,
        "one setting, one meaning, on both the sweep and the read model"
    );
}
