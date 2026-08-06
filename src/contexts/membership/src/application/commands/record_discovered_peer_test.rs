use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::application::commands::{RecordDiscoveredPeer, RecordDiscoveredPeerHandler};
use crate::domain::events::{MembershipEvent, PeerDiscovered};
use crate::domain::{
    DurationMillis, Endpoint, LivenessWindows, Millis, PeerRoster, PeerRosterError, Presence,
};
use crate::ports::port_fakes::{FailingPublisher, ManualClock, RecordingPublisher};
use crate::ports::{
    ClockPort, DiscoveredPeer, DiscoveryOutcome, EventPublisherError, EventPublisherPort,
    MembershipCommandError,
};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn observation(peer: PeerId, address: &str) -> DiscoveredPeer {
    DiscoveredPeer {
        peer,
        endpoints: vec![endpoint(address)],
    }
}

struct Fixture {
    state: Arc<MembershipState>,
    clock: Arc<ManualClock>,
    publisher: Arc<RecordingPublisher>,
    handler: RecordDiscoveredPeerHandler,
}

fn fixture() -> Fixture {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    let handler = RecordDiscoveredPeerHandler::new(
        Arc::clone(&state),
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    Fixture {
        state,
        clock,
        publisher,
        handler,
    }
}

#[test]
fn a_first_sighting_enters_the_roster_and_is_announced() {
    let f = fixture();

    let outcome = f
        .handler
        .handle(RecordDiscoveredPeer {
            discovered: observation(test_peers::bob(), "/ip4/198.51.100.7/udp/4001"),
        })
        .expect("recording another peer is legal");

    assert_eq!(
        outcome,
        DiscoveryOutcome::Recorded(PeerDiscovered {
            peer: test_peers::bob(),
            at: T0,
        })
    );
    assert_eq!(
        f.publisher.published(),
        vec![MembershipEvent::PeerDiscovered(PeerDiscovered {
            peer: test_peers::bob(),
            at: T0,
        })]
    );
    assert!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).is_some())
    );
}

#[test]
fn a_repeat_sighting_announces_nothing_and_is_not_evidence_of_life() {
    // The inversion of `a_repeat_sighting_refreshes_the_entry_...`, which
    // asserted that a re-announcement is evidence — the recurring half of the
    // defect, and the more dangerous half: it is the *only* one that can be
    // triggered on demand, by re-publishing a DHT record (canvas D3, A3b, S2,
    // S3).
    let f = fixture();
    let sighting = || RecordDiscoveredPeer {
        discovered: observation(test_peers::bob(), "/ip4/198.51.100.7/udp/4001"),
    };

    f.handler.handle(sighting()).expect("first sighting");
    f.clock.advance(DurationMillis::from_secs(5));
    let outcome = f.handler.handle(sighting()).expect("second sighting");

    assert_eq!(outcome, DiscoveryOutcome::Refreshed);
    assert!(outcome.is_known(), "the address was kept");
    assert_eq!(
        f.publisher.published().len(),
        1,
        "a gossiping network re-announces constantly; only the first sighting is news"
    );
    assert_eq!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).map(|e| e.last_seen_at())),
        Some(None),
        "neither sighting is the peer speaking, so bob has still produced nothing"
    );
    assert_eq!(
        f.state.read(|roster| roster
            .peer(&test_peers::bob())
            .map(|e| e.presence(f.clock.now(), LivenessWindows::DEFAULT))),
        Some(Presence::Unknown),
        "and no amount of being talked about moves a peer onto the liveness ladder"
    );
}

#[test]
fn re_announcing_a_peer_that_has_gone_quiet_cannot_revive_it() {
    // The Kademlia vector end to end at the application layer: bob really spoke
    // once, then stopped. A hostile host that keeps publishing DHT records
    // naming him must not be able to hold him `Online` here — nor to move his
    // evidence instant by a single millisecond (canvas D3, safeguard S5).
    let f = fixture();
    let sighting = || RecordDiscoveredPeer {
        discovered: observation(test_peers::bob(), "/ip4/198.51.100.7/udp/4001"),
    };

    f.handler.handle(sighting()).expect("first sighting");
    f.state.modify(|roster| {
        roster
            .record_heartbeat(test_peers::bob(), T0)
            .expect("bob speaks once, for real");
    });
    f.clock.advance(DurationMillis::from_secs(90));

    for _ in 0..10 {
        f.handler.handle(sighting()).expect("re-announcement");
        f.clock.advance(DurationMillis::from_secs(1));
    }

    assert_eq!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).map(|e| e.last_seen_at())),
        Some(Some(T0)),
        "the one instant bob acted, unmoved by ten claims that he exists"
    );
    assert_eq!(
        f.state.read(|roster| roster
            .peer(&test_peers::bob())
            .map(|e| e.presence(f.clock.now(), LivenessWindows::DEFAULT))),
        Some(Presence::Offline),
        "a peer that stopped speaking is offline however loudly it is announced"
    );
}

#[test]
fn a_roster_with_no_room_left_refuses_a_new_peer_without_calling_it_a_failure() {
    // Reaching the cap is what the cap is for, under exactly the load that
    // fills it. An `Err` here would make a caller that logs errors emit one per
    // sighting — an unbounded, attacker-inducible flood bought with a
    // bounded-memory defence (canvas D9, safeguard S5).
    let f = fixture();
    f.state.modify(|roster| {
        for (index, peer) in test_peers::synthetic(PeerRoster::MAX_PEERS)
            .into_iter()
            .enumerate()
        {
            let at = T0.saturating_add(DurationMillis::from_millis(index as u64));
            roster
                .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], at)
                .expect("discovery");
            // Evidence, so nothing in the roster is evictable.
            roster.record_heartbeat(peer, at).expect("heartbeat");
        }
    });
    let published_before = f.publisher.published().len();

    let outcome = f
        .handler
        .handle(RecordDiscoveredPeer {
            discovered: observation(test_peers::bob(), "/ip4/203.0.113.9/udp/4001"),
        })
        .expect("a full roster is a state, not a fault of the adapter that reported this");

    assert_eq!(outcome, DiscoveryOutcome::RosterFull);
    assert!(!outcome.is_new_peer());
    assert!(
        !outcome.is_known(),
        "nothing was recorded, and a caller must be able to tell that from Refreshed"
    );
    assert_eq!(outcome.event(), None);
    assert_eq!(
        f.publisher.published().len(),
        published_before,
        "no event, because nothing happened"
    );
    assert_eq!(
        f.state
            .read(|roster| (roster.len(), roster.peer(&test_peers::bob()).is_some())),
        (PeerRoster::MAX_PEERS, false),
        "the cap holds and no peer that had reached us was evicted to honour the sighting"
    );
}

#[test]
fn a_new_address_for_a_known_peer_is_merged() {
    let f = fixture();

    f.handler
        .handle(RecordDiscoveredPeer {
            discovered: observation(test_peers::bob(), "/ip4/198.51.100.7/udp/4001"),
        })
        .expect("first sighting");
    f.handler
        .handle(RecordDiscoveredPeer {
            discovered: observation(test_peers::bob(), "/ip4/203.0.113.9/udp/4001"),
        })
        .expect("second address");

    assert_eq!(
        f.state
            .read(|roster| roster.peer(&test_peers::bob()).map(|e| e.endpoints().len())),
        Some(2)
    );
}

#[test]
fn the_local_peers_own_announcement_is_a_normal_outcome_not_an_error() {
    // A peer's own announcement genuinely comes back from a gossiping network,
    // and its own join ticket can be pasted into the machine that minted it.
    let f = fixture();

    let outcome = f
        .handler
        .handle(RecordDiscoveredPeer {
            discovered: observation(test_peers::alice(), "/ip4/198.51.100.7/udp/4001"),
        })
        .expect("hearing yourself is not a fault");

    assert_eq!(outcome, DiscoveryOutcome::OwnAnnouncement);
    assert!(!outcome.is_new_peer());
    assert!(f.state.read(|roster| roster.is_empty()), "invariant 2");
    assert_eq!(f.publisher.published(), Vec::new());
}

#[test]
fn an_observation_with_no_address_to_dial_is_rejected() {
    let f = fixture();

    let outcome = f.handler.handle(RecordDiscoveredPeer {
        discovered: DiscoveredPeer {
            peer: test_peers::bob(),
            endpoints: Vec::new(),
        },
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Roster(PeerRosterError::NoEndpoints)),
        "an adapter reporting a peer with nowhere to reach it is reporting nothing"
    );
}

#[test]
fn a_publisher_failure_is_reported_rather_than_swallowed() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let handler = RecordDiscoveredPeerHandler::new(
        Arc::clone(&state),
        Arc::new(ManualClock::starting_at(T0)) as Arc<dyn ClockPort + Send + Sync>,
        Arc::new(FailingPublisher(EventPublisherError::Unavailable))
            as Arc<dyn EventPublisherPort + Send + Sync>,
    );

    let outcome = handler.handle(RecordDiscoveredPeer {
        discovered: observation(test_peers::bob(), "/ip4/198.51.100.7/udp/4001"),
    });

    assert_eq!(
        outcome,
        Err(MembershipCommandError::Publisher(
            EventPublisherError::Unavailable
        ))
    );
    assert!(
        state.read(|roster| roster.peer(&test_peers::bob()).is_some()),
        "the roster change happened; it is the announcement that did not"
    );
}
