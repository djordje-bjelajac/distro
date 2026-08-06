use std::sync::Arc;

use shared_types::PeerId;

use crate::application::MembershipState;
use crate::application::queries::{
    GetNetworkStatus, GetNetworkStatusHandler, GetNetworkView, GetNetworkViewHandler,
    ListKnownPeers, ListKnownPeersHandler,
};
use crate::domain::{
    DurationMillis, Endpoint, LivenessWindows, Millis, NetworkStatus, PeerStanding, Presence,
    SessionDirection, SessionState,
};
use crate::ports::port_fakes::{ManualClock, TickingClock};
use crate::ports::{ClockPort, KnownPeerView, NetworkView};
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);
const WINDOWS: LivenessWindows = LivenessWindows::DEFAULT;

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

fn empty_state() -> Arc<MembershipState> {
    Arc::new(MembershipState::for_local_peer(test_peers::alice()))
}

/// Records an address for `peer` and nothing else — the shape of a cache load,
/// an mDNS sighting, or a DHT record.
fn told_about(state: &Arc<MembershipState>, peer: PeerId, at: Millis) {
    state.modify(|roster| {
        roster
            .record_discovery(peer, vec![endpoint("/ip4/198.51.100.7/udp/4001")], at)
            .expect("discovery");
    });
}

/// The same, plus evidence that `peer` itself acted at `heard_from_at`.
fn heard_from(state: &Arc<MembershipState>, peer: PeerId, heard_from_at: Millis) {
    told_about(state, peer, heard_from_at);
    state.modify(|roster| {
        roster
            .record_heartbeat(peer, heard_from_at)
            .expect("the peer speaks");
    });
}

/// A completed handshake: a link *and* evidence, both at `at`.
fn linked_to(state: &Arc<MembershipState>, peer: PeerId, at: Millis) {
    told_about(state, peer, at);
    state.modify(|roster| {
        roster
            .open_session(peer, SessionDirection::Outbound, at)
            .expect("open");
        roster.establish_session(peer, at).expect("establish");
    });
}

fn handler_over(
    state: &Arc<MembershipState>,
    clock: Arc<dyn ClockPort + Send + Sync>,
) -> GetNetworkViewHandler {
    GetNetworkViewHandler::new(Arc::clone(state), clock, WINDOWS)
}

fn view_at(state: &Arc<MembershipState>, now: Millis) -> NetworkView {
    handler_over(state, Arc::new(ManualClock::starting_at(now))).handle(GetNetworkView)
}

fn row_for(view: &NetworkView, peer: PeerId) -> KnownPeerView {
    view.peers()
        .iter()
        .find(|row| row.peer == peer)
        .expect("peer has a row")
        .clone()
}

// ------------------------------------------------------------- one snapshot

#[test]
fn the_whole_view_comes_from_one_clock_reading() {
    // The direct proof. `TickingClock` counts, so a handler that reads the clock
    // for the status and again for the rows — or once per peer — is caught by
    // the count rather than by inspection.
    let state = empty_state();
    for peer in [test_peers::bob(), test_peers::carol(), test_peers::dave()] {
        heard_from(&state, peer, T0);
    }
    let clock = Arc::new(TickingClock::from(T0, DurationMillis::from_secs(45)));

    let view = handler_over(
        &state,
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
    )
    .handle(GetNetworkView);

    assert_eq!(
        clock.readings(),
        1,
        "one view, one instant: three peers must not cost three readings"
    );
    assert_eq!(
        view,
        view_at(&state, T0),
        "and the instant it used is the first one it was given, \
         so the view is exactly what a frozen clock at T0 produces"
    );
}

#[test]
fn peers_holding_identical_evidence_are_never_classified_differently() {
    // What the counter above would cost if it were wrong. Each reading of this
    // clock is 45 seconds after the last, which straddles the 30-second online
    // window: a per-peer reading would report the first peer `Online` and the
    // next `Stale` from evidence recorded at the very same instant, and a redraw
    // would show a boundary crossing that never happened.
    let state = empty_state();
    for peer in [test_peers::bob(), test_peers::carol(), test_peers::dave()] {
        heard_from(&state, peer, T0);
    }
    let clock = Arc::new(TickingClock::from(T0, DurationMillis::from_secs(45)));

    let view =
        handler_over(&state, clock as Arc<dyn ClockPort + Send + Sync>).handle(GetNetworkView);

    let presences: Vec<Presence> = view.peers().iter().map(|row| row.presence).collect();
    assert_eq!(
        presences,
        vec![Presence::Online; 3],
        "identical evidence, identical instant, identical verdict"
    );
}

#[test]
fn the_status_and_the_rows_are_read_off_one_classification() {
    // The observed screen, taken through the handler this time (canvas D5).
    // Two established links whose peers have gone quiet, one peer heard from
    // over somebody else's link and now equally quiet, and one peer nobody has
    // ever heard from.
    let state = empty_state();
    linked_to(&state, test_peers::bob(), T0);
    linked_to(&state, test_peers::carol(), T0);
    heard_from(&state, test_peers::dave(), T0);
    told_about(&state, test_peers::erin(), T0);

    let now = T0.saturating_add(DurationMillis::from_secs(61));
    let view = view_at(&state, now);

    assert_eq!(
        view.status(),
        NetworkStatus::from_connected_peers(2),
        "the two links are real and stay counted: suppressing them would hide \
         links a direct message can still be attempted over (S4)"
    );
    assert_eq!(
        view.status().connected_peers(),
        view.peers().iter().filter(|row| row.is_connected()).count(),
        "and the number is the number of rows that show as linked"
    );
    assert_eq!(
        row_for(&view, test_peers::bob()).standing(),
        PeerStanding::Linked(Presence::Offline)
    );
    assert_eq!(
        row_for(&view, test_peers::dave()).standing(),
        PeerStanding::Unlinked(Presence::Offline)
    );
    assert_ne!(
        row_for(&view, test_peers::bob()).standing(),
        row_for(&view, test_peers::dave()).standing(),
        "a counted peer is never the same value as an uncounted one"
    );
    assert_eq!(
        row_for(&view, test_peers::erin()).standing(),
        PeerStanding::Unlinked(Presence::Unknown),
        "and a peer nobody ever heard from is not offline; there is nothing to report"
    );
}

#[test]
fn the_view_agrees_with_the_narrower_queries_taken_at_the_same_instant() {
    // `network_view` is not a fourth answer. It is the same rows and the same
    // count, taken together — so a caller migrating from two calls to one must
    // not see the picture change.
    let state = empty_state();
    linked_to(&state, test_peers::bob(), T0);
    heard_from(&state, test_peers::carol(), T0);
    told_about(&state, test_peers::dave(), T0);

    for elapsed in [0, 31, 61, 600] {
        let now = T0.saturating_add(DurationMillis::from_secs(elapsed));
        let clock = Arc::new(ManualClock::starting_at(now)) as Arc<dyn ClockPort + Send + Sync>;
        let view = handler_over(&state, Arc::clone(&clock)).handle(GetNetworkView);

        let rows =
            ListKnownPeersHandler::new(Arc::clone(&state), clock, WINDOWS).handle(ListKnownPeers);
        let status = GetNetworkStatusHandler::new(Arc::clone(&state)).handle(GetNetworkStatus);

        assert_eq!(view.peers(), rows.as_slice(), "at +{elapsed}s");
        assert_eq!(view.status(), status, "at +{elapsed}s");
    }
}

// ------------------------------------------------------------------ joining

#[test]
fn a_join_in_flight_is_reported_as_joining_over_whatever_rows_exist() {
    // The one fact the roster cannot hold, and the one status not derived from
    // the rows. It outranks the count for the same reason it always has: a
    // re-join over live sessions is still a join, and the in-flight operation is
    // what the caller is waiting on.
    let state = empty_state();
    linked_to(&state, test_peers::bob(), T0);
    let handler = handler_over(&state, Arc::new(ManualClock::starting_at(T0)));

    let phase = state.begin_join();
    let joining = handler.handle(GetNetworkView);
    drop(phase);
    let settled = handler.handle(GetNetworkView);

    assert_eq!(joining.status(), NetworkStatus::Joining);
    assert_eq!(
        joining.peers(),
        settled.peers(),
        "the rows are the same rows; only the sentence above them changed"
    );
    assert_eq!(
        joining.standings(),
        vec![PeerStanding::Linked(Presence::Online)],
        "and each row still states its own case while the ladder walks"
    );
    assert_eq!(settled.status(), NetworkStatus::from_connected_peers(1));
}

// ------------------------------------------------------------- the read half

#[test]
fn a_peer_this_instance_was_only_told_about_gets_a_row_with_nothing_in_it() {
    // A cache load, an mDNS sighting and a DHT record all land here, and all
    // three are somebody else's claim (invariant 2). The peer is shown, because
    // it is a dialable candidate and hiding it turns "my peer vanished" into a
    // support question — but it is `Unknown`, at every age, until it answers.
    let state = empty_state();
    told_about(&state, test_peers::bob(), T0);

    for elapsed in [0, 31, 61, 3_600] {
        let view = view_at(
            &state,
            T0.saturating_add(DurationMillis::from_secs(elapsed)),
        );
        let bob = row_for(&view, test_peers::bob());

        assert_eq!(view.status(), NetworkStatus::Isolated, "at +{elapsed}s");
        assert_eq!(bob.presence, Presence::Unknown, "at +{elapsed}s");
        assert_eq!(bob.last_seen_at, None, "at +{elapsed}s");
        assert_eq!(bob.session, None, "at +{elapsed}s");
        assert!(!bob.is_connected(), "at +{elapsed}s");
    }

    state.modify(|roster| {
        roster
            .record_heartbeat(test_peers::bob(), T0)
            .expect("bob answers at last")
    });

    assert_eq!(
        row_for(&view_at(&state, T0), test_peers::bob()).presence,
        Presence::Online,
        "evidence is the only exit from Unknown"
    );
}

#[test]
fn a_dial_still_in_flight_is_not_yet_a_link_and_is_no_evidence_either() {
    let state = empty_state();
    told_about(&state, test_peers::bob(), T0);
    state.modify(|roster| {
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
    });

    let view = view_at(&state, T0);
    let bob = row_for(&view, test_peers::bob());

    assert_eq!(bob.session, Some(SessionState::Connecting));
    assert_eq!(
        bob.standing(),
        PeerStanding::Unlinked(Presence::Unknown),
        "our own dial demonstrates nothing about them, and a dial in flight \
         can carry nothing yet"
    );
    assert_eq!(view.status(), NetworkStatus::Isolated);
}

#[test]
fn taking_the_view_writes_nothing_however_often_it_is_taken() {
    let state = empty_state();
    linked_to(&state, test_peers::bob(), T0);
    heard_from(&state, test_peers::carol(), T0);
    told_about(&state, test_peers::dave(), T0);
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(
        &state,
        Arc::clone(&clock) as Arc<dyn ClockPort + Send + Sync>,
    );

    let before = state.read(Clone::clone);
    for _ in 0..5 {
        clock.advance(DurationMillis::from_secs(30));
        let _ = handler.handle(GetNetworkView);
    }
    let after = state.read(Clone::clone);

    assert_eq!(
        before, after,
        "a query path that mutated would make presence a fact someone set (invariant 7)"
    );
}

#[test]
fn an_instance_that_knows_nobody_reports_isolation_and_no_rows() {
    let view = view_at(&empty_state(), T0);

    assert_eq!(view.status(), NetworkStatus::Isolated);
    assert_eq!(view.peers(), &[]);
}
