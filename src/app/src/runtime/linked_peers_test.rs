use membership::domain::{Endpoint, Millis, Presence, SessionState};
use membership::ports::{KnownPeerView, NetworkView};
use shared_types::PeerId;

use crate::runtime::linked_peers;
use crate::test_peers::{alice, bob, carol};

fn view(peer: PeerId, presence: Presence, session: Option<SessionState>) -> KnownPeerView {
    KnownPeerView {
        peer,
        endpoints: vec![Endpoint::direct("/ip4/10.0.0.1/tcp/1").expect("a valid address")],
        presence,
        last_seen_at: match presence {
            Presence::Unknown => None,
            _ => Some(Millis::from_millis(1)),
        },
        session,
    }
}

#[test]
fn a_peer_holding_an_established_session_is_linked() {
    let snapshot = NetworkView::of(vec![view(
        bob(),
        Presence::Online,
        Some(SessionState::Established),
    )]);

    assert_eq!(linked_peers(&snapshot), vec![bob()]);
}

#[test]
fn a_peer_with_no_session_is_not_linked_however_alive_it_looks() {
    // A peer whose evidence arrived over somebody else's link cannot be sent a
    // direct message. Heartbeating it would fail on every tick and blame the
    // network for a set this root chose (canvas D4).
    let snapshot = NetworkView::of(vec![view(bob(), Presence::Online, None)]);

    assert!(linked_peers(&snapshot).is_empty());
}

#[test]
fn a_dial_in_flight_is_not_linked() {
    // The trap: a `Connecting` session is *live*, so a predicate written as
    // "holds a session" would send to a link that can carry nothing yet.
    let snapshot = NetworkView::of(vec![view(
        bob(),
        Presence::Online,
        Some(SessionState::Connecting),
    )]);

    assert!(linked_peers(&snapshot).is_empty());
}

#[test]
fn a_closed_session_is_not_linked() {
    let snapshot = NetworkView::of(vec![view(
        bob(),
        Presence::Online,
        Some(SessionState::Closed),
    )]);

    assert!(linked_peers(&snapshot).is_empty());
}

#[test]
fn a_never_heard_from_peer_holding_a_session_still_gets_one() {
    // `Unknown` is the absence of evidence, not a verdict — and a heartbeat is
    // exactly how the absence is resolved. Skipping these would leave a freshly
    // dialled peer permanently blank.
    let snapshot = NetworkView::of(vec![view(
        bob(),
        Presence::Unknown,
        Some(SessionState::Established),
    )]);

    assert_eq!(linked_peers(&snapshot), vec![bob()]);
}

#[test]
fn a_linked_but_silent_peer_still_gets_one() {
    // `Linked(Offline)` is the state the round trip exists to resolve: the link
    // is up and the peer is not answering. Giving up on it would make the state
    // permanent.
    let snapshot = NetworkView::of(vec![view(
        bob(),
        Presence::Offline,
        Some(SessionState::Established),
    )]);

    assert_eq!(linked_peers(&snapshot), vec![bob()]);
}

#[test]
fn only_the_linked_peers_of_a_mixed_roster_are_selected() {
    let snapshot = NetworkView::of(vec![
        view(bob(), Presence::Unknown, None),
        view(carol(), Presence::Stale, Some(SessionState::Established)),
        view(alice(), Presence::Online, Some(SessionState::Connecting)),
    ]);

    assert_eq!(linked_peers(&snapshot), vec![carol()]);
}

#[test]
fn an_empty_roster_selects_nobody() {
    assert!(linked_peers(&NetworkView::of(Vec::new())).is_empty());
}

#[test]
fn the_selection_is_exactly_what_the_status_line_counts() {
    // One classification, not two readings (canvas D5): if these could ever
    // differ, `connected (n)` would be a count of peers some of which this
    // instance never probes.
    let snapshot = NetworkView::of(vec![
        view(bob(), Presence::Unknown, None),
        view(carol(), Presence::Stale, Some(SessionState::Established)),
        view(alice(), Presence::Offline, Some(SessionState::Established)),
    ]);

    assert_eq!(
        snapshot.status().connected_peers(),
        linked_peers(&snapshot).len()
    );
    assert_eq!(snapshot.status().connected_peers(), 2);
}
