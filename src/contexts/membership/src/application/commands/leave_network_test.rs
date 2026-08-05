use std::sync::Arc;

use shared_types::{PeerDisconnected, PeerId};

use crate::application::MembershipState;
use crate::application::commands::{LeaveNetwork, LeaveNetworkHandler};
use crate::domain::events::{MembershipEvent, NetworkLeft};
use crate::domain::{DurationMillis, Endpoint, Millis, NetworkStatus, SessionDirection};
use crate::ports::port_fakes::{
    InMemoryPeerCache, ManualClock, RecordingPublisher, ScriptedTransport, UnusablePeerCache,
};
use crate::ports::{
    CachedPeer, ClockPort, EventPublisherPort, PeerCacheError, PeerCachePort, PeerTransportPort,
};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);
const BOB_ADDRESS: &str = "/ip4/198.51.100.7/udp/4001/quic-v1";
const CAROL_ADDRESS: &str = "/ip4/203.0.113.9/udp/4001/quic-v1";

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

struct Fixture {
    state: Arc<MembershipState>,
    clock: Arc<ManualClock>,
    transport: Arc<ScriptedTransport>,
    cache: Arc<dyn PeerCachePort + Send + Sync>,
    publisher: Arc<RecordingPublisher>,
}

impl Fixture {
    fn handler(&self) -> LeaveNetworkHandler {
        LeaveNetworkHandler::new(
            Arc::clone(&self.state),
            Arc::clone(&self.clock) as Arc<dyn ClockPort + Send + Sync>,
            Arc::clone(&self.transport) as Arc<dyn PeerTransportPort + Send + Sync>,
            Arc::clone(&self.cache),
            Arc::clone(&self.publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        )
    }

    fn leave(&self) -> crate::ports::LeaveOutcome {
        self.handler().handle(LeaveNetwork).expect("leave")
    }
}

/// `alice`, connected to `bob`, with `carol` known but never dialled.
fn fixture() -> Fixture {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let transport = Arc::new(
        ScriptedTransport::listening_on(Vec::new())
            .reachable_at(endpoint(BOB_ADDRESS))
            .reachable_at(endpoint(CAROL_ADDRESS)),
    );

    state.modify(|roster| {
        roster
            .record_discovery(test_peers::bob(), vec![endpoint(BOB_ADDRESS)], T0)
            .expect("discovery");
        roster
            .record_discovery(test_peers::carol(), vec![endpoint(CAROL_ADDRESS)], T0)
            .expect("discovery");
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
        roster
            .establish_session(test_peers::bob(), T0)
            .expect("establish");
    });
    transport
        .dial(test_peers::bob(), &[endpoint(BOB_ADDRESS)])
        .expect("the transport holds the link the roster describes");

    Fixture {
        state,
        clock: Arc::new(ManualClock::starting_at(T0)),
        transport,
        cache: Arc::new(InMemoryPeerCache::empty()),
        publisher: Arc::new(RecordingPublisher::new()),
    }
}

fn cache_contents(cache: &Arc<dyn PeerCachePort + Send + Sync>) -> Vec<CachedPeer> {
    cache.load().expect("in-memory cache reads")
}

fn peers_in(cached: &[CachedPeer]) -> Vec<PeerId> {
    cached.iter().map(|entry| entry.peer).collect()
}

#[test]
fn leaving_closes_every_session_and_announces_each_departure() {
    let f = fixture();
    f.clock.advance(DurationMillis::from_secs(30));

    let outcome = f.leave();

    assert_eq!(
        outcome.disconnected,
        vec![PeerDisconnected {
            peer: test_peers::bob()
        }]
    );
    assert_eq!(f.transport.closed(), vec![test_peers::bob()]);
    assert_eq!(f.state.read(|roster| roster.established_session_count()), 0);
    assert_eq!(f.state.network_status(), NetworkStatus::Isolated);
    assert_eq!(
        outcome.left,
        NetworkLeft {
            at: T0.saturating_add(DurationMillis::from_secs(30))
        }
    );
}

#[test]
fn the_departure_is_announced_after_the_sessions_that_ended() {
    let f = fixture();

    f.leave();

    assert_eq!(
        f.publisher.published(),
        vec![
            MembershipEvent::PeerDisconnected(PeerDisconnected {
                peer: test_peers::bob()
            }),
            MembershipEvent::NetworkLeft(NetworkLeft { at: T0 }),
        ],
        "a consumer must never see the network left while it still believes a link is live"
    );
}

#[test]
fn leaving_saves_every_known_peer_for_the_next_launchs_first_bootstrap_rung() {
    let f = fixture();

    let outcome = f.leave();

    let cached = cache_contents(&f.cache);
    assert_eq!(outcome.cached_peers, 2);
    let mut expected = vec![test_peers::bob(), test_peers::carol()];
    expected.sort_unstable();
    assert_eq!(
        peers_in(&cached),
        expected,
        "a peer that was never dialled is still an address worth keeping (D1 rung a)"
    );
    assert_eq!(outcome.cache_failure, None);
}

#[test]
fn what_is_cached_is_the_addresses_and_nothing_about_the_session() {
    let f = fixture();

    f.leave();

    let cached = cache_contents(&f.cache);
    let bob = cached
        .iter()
        .find(|entry| entry.peer == test_peers::bob())
        .expect("bob was cached");
    assert_eq!(bob.endpoints, vec![endpoint(BOB_ADDRESS)]);
    assert_eq!(
        bob.last_seen_at, T0,
        "the instant is what lets a cache prune peers dead for months"
    );
}

#[test]
fn a_cache_that_cannot_be_written_costs_a_warm_start_not_the_departure() {
    let mut f = fixture();
    f.cache = Arc::new(UnusablePeerCache(PeerCacheError::WriteFailed));

    let outcome = f.leave();

    assert_eq!(
        outcome.cache_failure,
        Some(PeerCacheError::WriteFailed),
        "stated, because a machine that silently stops warm-starting needs a ticket again"
    );
    assert_eq!(
        outcome.disconnected.len(),
        1,
        "the departure itself succeeded"
    );
}

#[test]
fn a_session_that_never_established_ends_without_being_announced() {
    let f = fixture();
    f.state.modify(|roster| {
        roster
            .open_session(test_peers::carol(), SessionDirection::Outbound, T0)
            .expect("carol is still handshaking");
    });

    let outcome = f.leave();

    assert_eq!(
        outcome.disconnected,
        vec![PeerDisconnected {
            peer: test_peers::bob()
        }],
        "no PeerConnected was ever published for carol, so no disconnect is either (D10)"
    );
    assert_eq!(
        f.state.read(|roster| roster
            .peer(&test_peers::carol())
            .and_then(|entry| entry.session().map(crate::domain::Session::state))),
        None,
        "the half-open link is still closed; it is only unannounced"
    );
}

#[test]
fn leaving_an_isolated_peer_is_a_no_op_that_still_saves_the_cache() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let f = Fixture {
        state,
        clock: Arc::new(ManualClock::starting_at(T0)),
        transport: Arc::new(ScriptedTransport::listening_on(Vec::new())),
        cache: Arc::new(InMemoryPeerCache::empty()),
        publisher: Arc::new(RecordingPublisher::new()),
    };

    let outcome = f.leave();

    assert_eq!(outcome.disconnected, Vec::new());
    assert_eq!(outcome.cached_peers, 0);
    assert_eq!(
        f.publisher.published(),
        vec![MembershipEvent::NetworkLeft(NetworkLeft { at: T0 })],
        "leaving is a local decision, and it was made"
    );
}

#[test]
fn the_peers_stay_known_after_leaving() {
    let f = fixture();

    f.leave();

    assert_eq!(
        f.state.read(|roster| roster.len()),
        2,
        "the roster is what gets written to the cache; forgetting it would erase the warm start"
    );
}
