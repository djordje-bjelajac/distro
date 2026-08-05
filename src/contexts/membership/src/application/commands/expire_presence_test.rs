use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::commands::{ExpirePresence, ExpirePresenceHandler};
use crate::domain::events::{MembershipEvent, PeerPresenceExpired};
use crate::domain::{DurationMillis, Endpoint, LivenessWindows, Millis, SessionDirection};
use crate::ports::port_fakes::{FailingPublisher, ManualClock, RecordingPublisher};
use crate::ports::{ClockPort, EventPublisherError, EventPublisherPort};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

struct Fixture {
    state: Arc<MembershipState>,
    clock: Arc<ManualClock>,
    publisher: Arc<RecordingPublisher>,
    handler: ExpirePresenceHandler,
}

/// A fixture knowing `bob` and `carol`, both last seen at `T0`.
fn fixture() -> Fixture {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    state.modify(|roster| {
        for peer in [test_peers::bob(), test_peers::carol()] {
            roster
                .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], T0)
                .expect("discovery");
        }
    });

    let handler = ExpirePresenceHandler::new(
        Arc::clone(&state),
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        LivenessWindows::DEFAULT,
    );

    Fixture {
        state,
        clock,
        publisher,
        handler,
    }
}

fn expired(f: &Fixture) -> Vec<PeerPresenceExpired> {
    f.handler.handle(ExpirePresence).expect("sweep")
}

#[test]
fn a_peer_that_has_not_been_heard_from_expires_within_the_liveness_window() {
    // AC5: stopping any instance leaves the others functional, and they
    // observe the departure within the liveness window.
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(61));
    let events = expired(&f);

    let now = T0.saturating_add(DurationMillis::from_secs(61));
    assert_eq!(
        events,
        vec![
            PeerPresenceExpired {
                peer: test_peers::bob(),
                last_evidence_at: T0,
                at: now,
            },
            PeerPresenceExpired {
                peer: test_peers::carol(),
                last_evidence_at: T0,
                at: now,
            },
        ],
        "reported in PeerId order, so a recorded trace is the same on every run (S5)"
    );
    assert_eq!(
        f.publisher.published().len(),
        2,
        "the expiry reaches whoever is listening, not just the caller"
    );
}

#[test]
fn nothing_expires_while_the_evidence_is_still_fresh() {
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(59));

    assert_eq!(
        expired(&f),
        Vec::new(),
        "a peer inside the offline window is not yet gone, only quiet"
    );
    assert_eq!(f.publisher.published(), Vec::new());
}

#[test]
fn a_sweep_that_finds_nothing_is_silent_however_often_it_runs() {
    let f = fixture();

    for _ in 0..10 {
        f.clock.advance(DurationMillis::from_secs(1));
        assert_eq!(expired(&f), Vec::new());
    }

    assert_eq!(f.publisher.published(), Vec::new());
}

#[test]
fn one_silence_produces_one_expiry_however_often_the_sweep_runs() {
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(61));
    let first = expired(&f);
    f.clock.advance(DurationMillis::from_secs(61));
    let second = expired(&f);

    assert_eq!(first.len(), 2);
    assert_eq!(
        second,
        Vec::new(),
        "the sweep is idempotent within one stretch of quiet"
    );
    assert_eq!(f.publisher.published().len(), 2);
}

#[test]
fn a_peer_that_comes_back_and_goes_quiet_again_expires_again() {
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(61));
    expired(&f);
    f.state.modify(|roster| {
        roster
            .record_heartbeat(test_peers::bob(), f.clock.now())
            .expect("bob speaks again");
    });
    f.clock.advance(DurationMillis::from_secs(61));
    let second = expired(&f);

    assert_eq!(
        second.iter().map(|event| event.peer).collect::<Vec<_>>(),
        vec![test_peers::bob()],
        "fresh evidence re-arms the expiry edge"
    );
}

#[test]
fn an_expiry_never_touches_the_session() {
    // Silence is not a close: only the transport can report a dead link, and
    // whether an expiry should provoke one is a decision, not an observation.
    let f = fixture();
    f.state.modify(|roster| {
        roster
            .open_session(test_peers::bob(), SessionDirection::Inbound, T0)
            .expect("open");
        roster
            .establish_session(test_peers::bob(), T0)
            .expect("establish");
    });

    f.clock.advance(DurationMillis::from_secs(61));
    let events = expired(&f);

    assert_eq!(events.len(), 2);
    assert_eq!(
        f.state.read(|roster| roster.established_session_count()),
        1,
        "the link is still open; the peer has simply stopped speaking on it"
    );
}

#[test]
fn the_expiry_carries_both_instants_because_it_is_a_statement_about_the_local_view() {
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(90));
    let events = expired(&f);

    let bob = events
        .iter()
        .find(|event| event.peer == test_peers::bob())
        .expect("bob expired");
    assert_eq!(bob.last_evidence_at, T0);
    assert_eq!(bob.at, T0.saturating_add(DurationMillis::from_secs(90)));
}

#[test]
fn shorter_windows_expire_a_peer_sooner_without_any_other_change() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0));
    let publisher = Arc::new(RecordingPublisher::new());
    state.modify(|roster| {
        roster
            .record_discovery(
                test_peers::bob(),
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                T0,
            )
            .expect("discovery");
    });
    let windows = LivenessWindows::new(
        DurationMillis::from_millis(100),
        DurationMillis::from_millis(200),
    )
    .expect("online is shorter than offline");
    let handler = ExpirePresenceHandler::new(
        Arc::clone(&state),
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::clone(&publisher) as Arc<dyn EventPublisherPort + Send + Sync>,
        windows,
    );

    clock.advance(DurationMillis::from_millis(200));
    let events = handler.handle(ExpirePresence).expect("sweep");

    assert_eq!(
        events.len(),
        1,
        "a deterministic scenario need not wait a minute"
    );
}

#[test]
fn a_publisher_failure_is_reported_rather_than_swallowed() {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    let clock = Arc::new(ManualClock::starting_at(T0));
    state.modify(|roster| {
        roster
            .record_discovery(
                test_peers::bob(),
                vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                T0,
            )
            .expect("discovery");
    });
    let handler = ExpirePresenceHandler::new(
        state,
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
        Arc::new(FailingPublisher(EventPublisherError::Unavailable))
            as Arc<dyn EventPublisherPort + Send + Sync>,
        LivenessWindows::DEFAULT,
    );

    clock.advance(DurationMillis::from_secs(61));

    assert_eq!(
        handler.handle(ExpirePresence),
        Err(EventPublisherError::Unavailable)
    );
}

#[test]
fn the_events_reach_the_publisher_as_membership_events() {
    let f = fixture();

    f.clock.advance(DurationMillis::from_secs(61));
    let events = expired(&f);

    assert_eq!(
        f.publisher.published(),
        events
            .iter()
            .map(|event| MembershipEvent::PeerPresenceExpired(*event))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        f.publisher.cross_context(),
        Vec::new(),
        "presence is this context's own business; no other context learns what it is"
    );
}
