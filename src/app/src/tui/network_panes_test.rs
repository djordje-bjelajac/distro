use std::cell::Cell;

use membership::domain::{Endpoint, Millis, NetworkStatus, Presence, SessionState};
use membership::ports::{KnownPeerView, MembershipQueryPort, NetworkView};
use shared_types::PeerId;

use crate::composition::PeerTrust;
use crate::test_peers::{alice, bob, carol};
use crate::tui::{NetworkPanes, PeerLabels};

/// What the fake says when a second reading is taken.
///
/// Not an assertion helper: the point is that the frame's network path calls
/// `network_view` and nothing else. A test that merely counted calls would
/// still pass if a later change read `known_peers` for one more pane, which is
/// precisely how the two panes came to describe different rosters.
const SECOND_READING: &str =
    "the status line and the roster must come from one snapshot: this is a second reading";

/// A query port holding one snapshot, which refuses to be read twice.
struct OneSnapshot {
    view: NetworkView,
    readings: Cell<usize>,
}

impl OneSnapshot {
    fn of(peers: Vec<KnownPeerView>) -> Self {
        Self {
            view: NetworkView::of(peers),
            readings: Cell::new(0),
        }
    }

    fn readings(&self) -> usize {
        self.readings.get()
    }
}

impl MembershipQueryPort for OneSnapshot {
    fn network_view(&self) -> NetworkView {
        self.readings.set(self.readings.get() + 1);
        self.view.clone()
    }

    fn known_peers(&self) -> Vec<KnownPeerView> {
        panic!("{SECOND_READING}")
    }

    fn online_peers(&self) -> Vec<PeerId> {
        panic!("{SECOND_READING}")
    }

    fn network_status(&self) -> NetworkStatus {
        panic!("{SECOND_READING}")
    }
}

fn view(peer: PeerId, presence: Presence, session: Option<SessionState>) -> KnownPeerView {
    KnownPeerView {
        peer,
        endpoints: vec![Endpoint::direct("/ip4/10.0.0.1/tcp/1").expect("a valid address")],
        presence,
        last_seen_at: if presence.is_unknown() {
            None
        } else {
            Some(Millis::from_millis(1))
        },
        session,
    }
}

fn untrusted(_peer: PeerId) -> PeerTrust {
    PeerTrust::default()
}

fn panes(peers: Vec<KnownPeerView>) -> NetworkPanes {
    NetworkPanes::gather(
        &OneSnapshot::of(peers),
        PeerLabels::for_local(alice()),
        untrusted,
    )
}

/// Words that state a peer is not there. A row the status line is counting may
/// never be one of them on its own (canvas D5's amended A5).
const ABSENCE_WORDS: [&str; 5] = ["offline", "unknown", "gone", "absent", "disconnected"];

/// Every session state crossed with every presence: the twelve rows a roster
/// can be made of.
const EVERY_COMBINATION: [(Option<SessionState>, Presence); 12] = [
    (None, Presence::Unknown),
    (None, Presence::Online),
    (None, Presence::Stale),
    (None, Presence::Offline),
    (Some(SessionState::Connecting), Presence::Unknown),
    (Some(SessionState::Connecting), Presence::Online),
    (Some(SessionState::Connecting), Presence::Stale),
    (Some(SessionState::Connecting), Presence::Offline),
    (Some(SessionState::Established), Presence::Unknown),
    (Some(SessionState::Established), Presence::Online),
    (Some(SessionState::Established), Presence::Stale),
    (Some(SessionState::Established), Presence::Offline),
];

#[test]
fn the_status_line_and_the_roster_come_from_one_snapshot() {
    // One reading, and the fake panics on any other — so this fails both if the
    // count is taken separately and if a later pane reaches for the roster
    // again.
    let queries = OneSnapshot::of(vec![
        view(bob(), Presence::Offline, Some(SessionState::Established)),
        view(carol(), Presence::Unknown, None),
    ]);

    let panes = NetworkPanes::gather(&queries, PeerLabels::for_local(alice()), untrusted);

    assert_eq!(queries.readings(), 1);
    assert_eq!(panes.status().connected_peers(), 1);
    assert_eq!(panes.roster().len(), 2);
}

#[test]
fn the_count_is_the_number_of_rows_that_draw_as_linked() {
    let panes = panes(vec![
        view(bob(), Presence::Offline, Some(SessionState::Established)),
        view(carol(), Presence::Online, Some(SessionState::Established)),
        view(alice(), Presence::Online, None),
    ]);

    assert_eq!(
        panes.status().connected_peers(),
        panes.roster().iter().filter(|row| row.connected()).count()
    );
    assert_eq!(panes.status().to_string(), "connected (2 peers)");
}

#[test]
fn the_observed_screen_is_no_longer_what_this_state_draws() {
    // The defect, reproduced: two peers holding established sessions whose
    // evidence has aged past the offline window. The status line said
    // `connected (2 peers)` and every row said `offline`, on two instances at
    // once.
    //
    // Both facts are still stated — the count is not suppressed, and no peer is
    // claimed to be online — but the row now says which one it is (canvas D5,
    // safeguard S4).
    let panes = panes(vec![
        view(bob(), Presence::Offline, Some(SessionState::Established)),
        view(carol(), Presence::Offline, Some(SessionState::Established)),
    ]);

    assert_eq!(panes.status().to_string(), "connected (2 peers)");
    for row in panes.roster() {
        assert_eq!(row.presence_text(), "connected · not answering");
        assert_ne!(row.presence_text(), Presence::Offline.to_string());
        // The invariant is not weakened to get there: the peer is still offline
        // by the evidence, and still counted by the link.
        assert!(row.presence().is_offline());
        assert!(row.connected());
    }
}

#[test]
fn no_row_counted_in_connected_is_a_bare_absence_word() {
    // The A5 property, over every roster that can be built from the twelve
    // combinations — 4096 of them, including the empty one. This is the test
    // that would have caught the screenshot.
    for roster in 0u32..(1 << EVERY_COMBINATION.len()) {
        let peers: Vec<KnownPeerView> = EVERY_COMBINATION
            .iter()
            .enumerate()
            .filter(|(index, _)| roster & (1 << index) != 0)
            .map(|(_, (session, presence))| view(bob(), *presence, *session))
            .collect();

        let panes = panes(peers);
        let counted = panes.status().connected_peers();
        let linked: Vec<_> = panes
            .roster()
            .iter()
            .filter(|row| row.connected())
            .collect();

        // One derivation: the number in the status line *is* the number of rows
        // that draw as linked, for every roster.
        assert_eq!(counted, linked.len(), "roster {roster:#014b}");

        for row in linked {
            assert!(
                !ABSENCE_WORDS.contains(&row.presence_text()),
                "a peer counted in {} rendered {:?}",
                panes.status(),
                row.presence_text()
            );
        }
    }
}

#[test]
fn a_never_heard_from_peer_is_in_the_roster_with_a_blank_cell_and_is_not_counted() {
    // The state most rows are in after a cache load: an address we hold and
    // have never reached. It is shown — it is a dialable candidate — and it
    // claims nothing, in either direction.
    let panes = panes(vec![view(bob(), Presence::Unknown, None)]);

    assert_eq!(panes.roster().len(), 1);
    assert_eq!(panes.roster()[0].presence_text(), "");
    assert!(panes.roster()[0].presence().is_unknown());
    assert!(!panes.roster()[0].presence().is_offline());
    assert_eq!(panes.status(), NetworkStatus::Isolated);
}

#[test]
fn the_conversation_list_names_the_peers_the_roster_shows() {
    // Both come off the same snapshot, so a peer cannot be in one pane and
    // missing from the other within a frame.
    let panes = panes(vec![
        view(bob(), Presence::Unknown, None),
        view(carol(), Presence::Online, Some(SessionState::Established)),
    ]);

    assert_eq!(panes.peer_ids(), vec![bob(), carol()]);
    assert_eq!(
        panes.peer_ids(),
        panes
            .roster()
            .iter()
            .map(|row| row.peer)
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_empty_roster_is_isolated_and_draws_nothing() {
    let panes = panes(Vec::new());

    assert_eq!(panes.status(), NetworkStatus::Isolated);
    assert!(panes.roster().is_empty());
    assert!(panes.peer_ids().is_empty());
}
