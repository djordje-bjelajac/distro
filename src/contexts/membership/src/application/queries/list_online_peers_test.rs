use std::sync::Arc;

use crate::application::MembershipState;
use crate::application::queries::{ListOnlinePeers, ListOnlinePeersHandler};
use crate::domain::{DurationMillis, Endpoint, LivenessWindows, Millis, SessionDirection};
use crate::ports::ClockPort;
use crate::ports::port_fakes::ManualClock;
use crate::test_peers;

const T0: Millis = Millis::from_millis(1_000);

fn endpoint(address: &str) -> Endpoint {
    Endpoint::direct(address).expect("test address is well formed")
}

/// A state holding an address for each peer and **no evidence about any of
/// them**: the shape of a roster just after a cache load, an mDNS sweep, or a
/// batch of DHT records.
fn state_told_about(peers: &[shared_types::PeerId]) -> Arc<MembershipState> {
    let state = Arc::new(MembershipState::for_local_peer(test_peers::alice()));
    state.modify(|roster| {
        for (index, peer) in peers.iter().enumerate() {
            roster
                .record_discovery(
                    *peer,
                    vec![endpoint("/ip4/198.51.100.7/udp/4001")],
                    T0.saturating_add(DurationMillis::from_millis(index as u64)),
                )
                .expect("discovery");
        }
    });
    state
}

/// The same, after each peer has itself spoken at the given instant.
///
/// The two steps are separate on purpose. This helper used to name its
/// parameter `seen_at` and hand it straight to `record_discovery`, so every
/// test in this file measured the age of an instant no peer had produced — the
/// helper *was* the defect, and every assertion built on it was really an
/// assertion that being talked about makes a peer online.
fn state_having_heard(peers: &[(shared_types::PeerId, Millis)]) -> Arc<MembershipState> {
    let state = state_told_about(&peers.iter().map(|(peer, _)| *peer).collect::<Vec<_>>());
    state.modify(|roster| {
        for (peer, heard_from_at) in peers {
            roster
                .record_heartbeat(*peer, *heard_from_at)
                .expect("the peer speaks for itself");
        }
    });
    state
}

fn handler_over(state: &Arc<MembershipState>, clock: &Arc<ManualClock>) -> ListOnlinePeersHandler {
    ListOnlinePeersHandler::new(
        Arc::clone(state),
        Arc::clone(clock) as Arc<dyn ClockPort + Send + Sync>,
        LivenessWindows::DEFAULT,
    )
}

#[test]
fn a_peer_with_fresh_evidence_is_online() {
    let state = state_having_heard(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    assert_eq!(
        handler_over(&state, &clock).handle(ListOnlinePeers),
        vec![test_peers::bob()]
    );
}

#[test]
fn a_peer_this_instance_was_only_told_about_is_never_online() {
    // A9/A3 at the read model: an entry restored from the peer cache, or learned
    // from mDNS or the DHT, holds an address and nothing else. It is a candidate
    // worth dialling — it stays in the roster — but "who is around" is a
    // question about evidence, and there is none. The list stays empty at every
    // instant, and the peer joins it only when it answers (canvas D1, D3, A3b).
    let state = state_told_about(&[test_peers::bob(), test_peers::carol()]);
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    for _ in 0..8 {
        assert_eq!(
            handler.handle(ListOnlinePeers),
            Vec::new(),
            "an address is not an answer, at any age"
        );
        clock.advance(DurationMillis::from_secs(10));
    }
    assert_eq!(
        state.read(|roster| roster.len()),
        2,
        "both are still known: they are dialable, just not vouched for"
    );

    state.modify(|roster| {
        roster
            .record_heartbeat(test_peers::bob(), clock.now())
            .expect("bob finally answers");
    });

    assert_eq!(
        handler.handle(ListOnlinePeers),
        vec![test_peers::bob()],
        "one peer answered, and exactly one peer is reported"
    );
}

#[test]
fn a_peer_whose_evidence_aged_out_drops_off_the_list_without_any_write() {
    let state = state_having_heard(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));
    let handler = handler_over(&state, &clock);

    clock.advance(DurationMillis::from_secs(31));

    assert_eq!(
        handler.handle(ListOnlinePeers),
        Vec::new(),
        "presence is derived from evidence age, not asserted by anyone (invariant 7)"
    );
}

#[test]
fn online_is_about_evidence_of_life_not_about_holding_a_session() {
    let state = state_having_heard(&[(test_peers::bob(), T0)]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    // A peer that was seen but never dialled is online and not connected;
    // the two questions have different answers and different queries.
    state.modify(|roster| {
        roster
            .open_session(test_peers::bob(), SessionDirection::Outbound, T0)
            .expect("open");
    });

    assert_eq!(
        handler_over(&state, &clock).handle(ListOnlinePeers),
        vec![test_peers::bob()]
    );
    assert_eq!(state.read(|roster| roster.established_session_count()), 0);
}

#[test]
fn the_list_is_ordered_by_peer_id() {
    let state = state_having_heard(&[
        (test_peers::erin(), T0),
        (test_peers::bob(), T0),
        (test_peers::carol(), T0),
    ]);
    let clock = Arc::new(ManualClock::starting_at(T0));

    let online = handler_over(&state, &clock).handle(ListOnlinePeers);

    let mut expected = vec![test_peers::erin(), test_peers::bob(), test_peers::carol()];
    expected.sort_unstable();
    assert_eq!(online, expected);
}
