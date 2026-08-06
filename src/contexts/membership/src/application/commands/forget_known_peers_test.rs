use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::application::commands::{
    ForgetKnownPeers, ForgetKnownPeersHandler, LeaveNetwork, LeaveNetworkHandler,
};
use crate::domain::{Endpoint, Millis, SessionDirection};
use crate::ports::port_fakes::{
    InMemoryPeerCache, ManualClock, RecordingPublisher, ScriptedTransport, UnusablePeerCache,
};
use crate::ports::{
    ClockPort, EventPublisherPort, ForgetPeersError, ForgetPeersOutcome, PeerCacheError,
    PeerCachePort, PeerTransportPort,
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
    cache: Arc<InMemoryPeerCache>,
    publisher: Arc<RecordingPublisher>,
}

impl Fixture {
    fn leave_handler_over(
        &self,
        cache: Arc<dyn PeerCachePort + Send + Sync>,
    ) -> LeaveNetworkHandler {
        LeaveNetworkHandler::new(
            Arc::clone(&self.state),
            Arc::clone(&self.clock) as Arc<dyn ClockPort + Send + Sync>,
            Arc::clone(&self.transport) as Arc<dyn PeerTransportPort + Send + Sync>,
            cache,
            Arc::clone(&self.publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        )
    }

    fn handler(&self) -> ForgetKnownPeersHandler {
        let cache = Arc::clone(&self.cache) as Arc<dyn PeerCachePort + Send + Sync>;
        ForgetKnownPeersHandler::new(
            Arc::clone(&self.state),
            Arc::clone(&cache),
            self.leave_handler_over(cache),
        )
    }

    /// The same handler, but with a cache that refuses every write — the
    /// partial-failure case.
    fn handler_with_unusable_cache(&self) -> ForgetKnownPeersHandler {
        let cache: Arc<dyn PeerCachePort + Send + Sync> =
            Arc::new(UnusablePeerCache(PeerCacheError::WriteFailed));
        ForgetKnownPeersHandler::new(
            Arc::clone(&self.state),
            Arc::clone(&cache),
            self.leave_handler_over(cache),
        )
    }

    fn forget(&self) -> ForgetPeersOutcome {
        self.handler()
            .handle(ForgetKnownPeers)
            .expect("forgetting succeeds when no join is running")
    }

    fn leave(&self) {
        self.leave_handler_over(Arc::clone(&self.cache) as Arc<dyn PeerCachePort + Send + Sync>)
            .handle(LeaveNetwork)
            .expect("leave");
    }

    fn roster_size(&self) -> usize {
        self.state.read(|roster| roster.len())
    }
}

/// `alice`, in an established session with `bob`, having also heard from
/// `carol` — two peers with evidence, so both are cacheable.
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
            .record_heartbeat(test_peers::carol(), T0)
            .expect("carol has spoken, so she is cacheable");
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

fn cached_peers(cache: &InMemoryPeerCache) -> Vec<PeerId> {
    cache
        .load()
        .expect("in-memory cache reads")
        .iter()
        .map(|entry| entry.peer)
        .collect()
}

// ------------------------------------------------------------- the operation

#[test]
fn forgetting_empties_the_roster_and_reports_how_many_went() {
    let f = fixture();

    let outcome = f.forget();

    assert_eq!(outcome.forgotten, 2);
    assert_eq!(f.roster_size(), 0);
    assert!(f.state.read(|roster| roster.is_empty()));
}

#[test]
fn forgetting_leaves_the_cache_empty_on_disk() {
    let f = fixture();

    f.forget();

    assert!(cached_peers(&f.cache).is_empty());
}

/// The write-back trap, stated as an ordering claim rather than an end state.
///
/// A handler that empties the file and stops looks correct here — until a quit
/// leaves the network and rewrites the cache from a roster nobody emptied. The
/// only way to tell a correct implementation from that one is the *order* of
/// the writes: the populated save a leave performs must come first and the
/// empty one must come last.
#[test]
fn the_empty_write_is_the_last_one_the_cache_sees() {
    let f = fixture();

    f.forget();

    let history = f.cache.write_history();
    assert_eq!(history.len(), 2, "a leave writes, then the forget writes");
    assert_eq!(
        history[0],
        vec![test_peers::bob(), test_peers::carol()],
        "the leave saved the roster it still had"
    );
    assert!(
        history[1].is_empty(),
        "and the forget overwrote it with nothing"
    );
}

/// The other half of the trap: quitting after a forget must not resurrect
/// anything, because there is no longer a roster to resurrect it from.
#[test]
fn quitting_after_a_forget_does_not_write_the_forgotten_peers_back() {
    let f = fixture();

    f.forget();
    f.leave();

    assert!(cached_peers(&f.cache).is_empty());
    assert!(
        f.cache
            .write_history()
            .last()
            .expect("the leave wrote")
            .is_empty()
    );
}

#[test]
fn forgetting_closes_every_live_session_before_the_roster_is_emptied() {
    let f = fixture();

    let outcome = f.forget();

    assert_eq!(
        outcome
            .disconnected
            .iter()
            .map(|event| event.peer)
            .collect::<Vec<_>>(),
        vec![test_peers::bob()],
        "the established session was announced as it closed"
    );
    assert_eq!(
        f.transport.closed(),
        vec![test_peers::bob()],
        "and the transport was told to drop the link — a link outliving the \
         roster entry is what lets a forgotten peer come straight back"
    );
}

// ------------------------------------------------------------------ refusals

#[test]
fn forgetting_is_refused_while_a_join_is_in_flight_and_changes_nothing() {
    let f = fixture();

    let phase = f.state.begin_join();
    let refusal = f.handler().handle(ForgetKnownPeers);
    drop(phase);

    assert_eq!(refusal, Err(ForgetPeersError::JoinInFlight));
    assert_eq!(f.roster_size(), 2, "the roster is untouched");
    assert_eq!(f.cache.write_history().len(), 0, "and nothing was written");
}

/// The roster cannot fail to empty; the file can refuse. Both facts are owed
/// to the user, because "they will be back next launch" is the actionable half.
#[test]
fn a_cache_that_refuses_is_reported_beside_the_peers_that_were_forgotten() {
    let f = fixture();

    let outcome = f
        .handler_with_unusable_cache()
        .handle(ForgetKnownPeers)
        .expect("a cache failure does not fail the forget");

    assert_eq!(outcome.forgotten, 2);
    assert_eq!(outcome.cache_failure, Some(PeerCacheError::WriteFailed));
    assert_eq!(f.roster_size(), 0);
}

// --------------------------------------------------------------- idempotence

/// Forgetting twice is not an error and not a surprise. A user who presses the
/// key again because nothing visible changed the first time must not get a
/// failure for it.
#[test]
fn forgetting_a_roster_that_is_already_empty_is_a_no_op_that_still_clears_the_cache() {
    let f = fixture();
    f.forget();

    let second = f.forget();

    assert_eq!(second.forgotten, 0);
    assert!(second.disconnected.is_empty());
    assert!(cached_peers(&f.cache).is_empty());
}
