use shared_types::PeerId;

use crate::domain::{Endpoint, NetworkStatus, PeerStanding, Presence, SessionState};
use crate::ports::{KnownPeerView, NetworkView};
use crate::test_peers;

fn endpoint() -> Endpoint {
    Endpoint::direct("/ip4/198.51.100.7/udp/4001/quic-v1").expect("test address is well formed")
}

/// Every shape a row can take: the whole product of the two facts a standing is
/// derived from.
const SHAPES: [(Option<SessionState>, Presence); 16] = [
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
    (Some(SessionState::Closed), Presence::Unknown),
    (Some(SessionState::Closed), Presence::Online),
    (Some(SessionState::Closed), Presence::Stale),
    (Some(SessionState::Closed), Presence::Offline),
];

fn row(peer: PeerId, shape: (Option<SessionState>, Presence)) -> KnownPeerView {
    KnownPeerView {
        peer,
        endpoints: vec![endpoint()],
        presence: shape.1,
        last_seen_at: None,
        session: shape.0,
    }
}

/// Rows in every combination of `size` shapes, with `size` distinct peers.
fn every_snapshot_of(size: usize) -> impl Iterator<Item = Vec<KnownPeerView>> {
    let peers = test_peers::synthetic(size);
    let combinations = SHAPES.len().pow(size as u32);

    (0..combinations).map(move |mut index| {
        peers
            .iter()
            .map(|peer| {
                let shape = SHAPES[index % SHAPES.len()];
                index /= SHAPES.len();
                row(*peer, shape)
            })
            .collect()
    })
}

#[test]
fn the_count_is_the_number_of_rows_that_render_as_linked() {
    // Exhaustive rather than sampled: `NetworkView::of` is a pure function of
    // the rows it is handed, and there are sixteen row shapes, so enumerating
    // every snapshot of up to three rows is a complete proof over that space
    // — no seed, no generator to under-cover a case (canvas D5).
    //
    // This is the property the observed screen violated. The status line said
    // `connected (2 peers)` while every row read `offline`, because the count
    // and the rows were two derivations that nothing tied together.
    for size in 0..=3 {
        for peers in every_snapshot_of(size) {
            let expected_linked = peers.iter().filter(|view| view.is_connected()).count();
            let view = NetworkView::of(peers);

            assert_eq!(
                view.status().connected_peers(),
                expected_linked,
                "status {} disagrees with its own rows: {:?}",
                view.status(),
                view.standings()
            );
            assert_eq!(
                view.status(),
                NetworkStatus::from_standings(&view.standings()),
                "the count is not read off the standings it reports"
            );
        }
    }
}

#[test]
fn a_counted_row_is_never_the_same_value_as_an_uncounted_one() {
    // The achievable half of A5 (canvas D5, safeguard S4). `Linked(Offline)`
    // must survive to the renderer as a value distinct from
    // `Unlinked(Offline)`: a working link to a peer that is not answering is not
    // the same statement as no link at all, and it is the row that would
    // otherwise be a bare absence word underneath a non-zero count.
    for size in 0..=3 {
        for peers in every_snapshot_of(size) {
            let view = NetworkView::of(peers);

            for standing in view.standings() {
                if standing.is_linked() {
                    assert_ne!(
                        standing,
                        PeerStanding::Unlinked(standing.presence()),
                        "a counted peer collapsed into an unlinked row"
                    );
                }
            }
        }
    }
}

#[test]
fn every_row_keeps_its_own_standing_alongside_the_count() {
    for size in 0..=3 {
        for peers in every_snapshot_of(size) {
            let expected: Vec<PeerStanding> = peers.iter().map(KnownPeerView::standing).collect();
            let view = NetworkView::of(peers);

            assert_eq!(view.standings(), expected);
            assert_eq!(
                view.peers().len(),
                expected.len(),
                "every peer has a row, including the ones that count for nothing"
            );
        }
    }
}

#[test]
fn a_view_of_nothing_is_isolated_rather_than_a_count_of_zero() {
    let view = NetworkView::of(Vec::new());

    assert_eq!(view.status(), NetworkStatus::Isolated);
    assert_eq!(view.peers(), &[]);
    assert!(
        view.status().is_isolated(),
        "a fresh install with no cached peers, no LAN neighbour and no ticket \
         is isolated by definition, and that is a state and not a failure"
    );
}

#[test]
fn a_never_heard_from_peer_is_shown_and_is_counted_as_nothing() {
    // The wording decision of canvas §3: such peers stay in the roster, because
    // they are dialable candidates and hiding them turns "my peer vanished" into
    // a support question. What they must not do is influence the count.
    let view = NetworkView::of(vec![
        row(test_peers::bob(), (None, Presence::Unknown)),
        row(test_peers::carol(), (None, Presence::Unknown)),
    ]);

    assert_eq!(view.status(), NetworkStatus::Isolated);
    assert_eq!(view.peers().len(), 2, "both are shown");
    assert_eq!(
        view.standings(),
        vec![
            PeerStanding::Unlinked(Presence::Unknown),
            PeerStanding::Unlinked(Presence::Unknown),
        ]
    );
}

#[test]
fn joining_outranks_the_count_without_claiming_anything_about_a_row() {
    // `Joining` is not a count, so it is the one status not derived from the
    // rows — and it asserts nothing about them. Each row still carries its own
    // standing, so a renderer showing "joining" above an established link is
    // stating two true things rather than contradicting itself.
    let peers = vec![
        row(
            test_peers::bob(),
            (Some(SessionState::Established), Presence::Online),
        ),
        row(test_peers::carol(), (None, Presence::Unknown)),
    ];
    let view = NetworkView::joining(peers.clone());

    assert_eq!(view.status(), NetworkStatus::Joining);
    assert_eq!(
        view.status().connected_peers(),
        0,
        "a join in flight counts nobody, so no row is claimed to be reachable"
    );
    assert_eq!(view.peers(), peers.as_slice());
    assert_eq!(
        view.standings()[0],
        PeerStanding::Linked(Presence::Online),
        "the row's own statement is untouched by the status above it"
    );
}

#[test]
fn the_parts_a_renderer_takes_are_the_ones_it_was_shown() {
    let peers = vec![row(
        test_peers::bob(),
        (Some(SessionState::Established), Presence::Offline),
    )];
    let view = NetworkView::of(peers.clone());
    let status = view.status();
    let standings = view.standings();

    let (taken_status, taken_peers) = view.into_parts();

    assert_eq!(taken_status, status);
    assert_eq!(taken_peers, peers);
    assert_eq!(standings, vec![PeerStanding::Linked(Presence::Offline)]);
    assert_eq!(
        taken_status,
        NetworkStatus::from_connected_peers(1),
        "the link is real and stays counted even though the peer is silent"
    );
}
