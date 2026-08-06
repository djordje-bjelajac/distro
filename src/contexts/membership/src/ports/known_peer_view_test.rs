use std::collections::HashSet;

use shared_types::PeerId;

use crate::domain::{
    DurationMillis, Endpoint, KnownPeer, LivenessWindows, Millis, NetworkStatus, PeerRoster,
    PeerStanding, Presence, SessionDirection, SessionState,
};
use crate::ports::KnownPeerView;
use crate::test_peers;

/// Well away from the origin, so an "age" can never be confused with an
/// absolute reading.
const T0: Millis = Millis::from_millis(1_000_000);

const WINDOWS: LivenessWindows = LivenessWindows::DEFAULT;

fn endpoint() -> Endpoint {
    Endpoint::direct("/ip4/198.51.100.7/udp/4001/quic-v1").expect("test address is well formed")
}

fn later(millis: u64) -> Millis {
    T0.saturating_add(DurationMillis::from_millis(millis))
}

fn view_of(roster: &PeerRoster, peer: PeerId, now: Millis) -> KnownPeerView {
    KnownPeerView::of(
        roster.peer(&peer).expect("peer is in the roster"),
        now,
        WINDOWS,
    )
}

// -------------------------------------------------------------- projection

#[test]
fn a_peer_never_heard_from_projects_no_evidence_instant() {
    // The state every entry starts in. There is no instant to report, and the
    // one instant available — when we were *told about* the peer — is exactly
    // the fabricated input the canvas removes (D1, D3).
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    roster
        .record_discovery(test_peers::bob(), vec![endpoint()], T0)
        .unwrap();

    let view = view_of(&roster, test_peers::bob(), later(500_000));

    assert_eq!(view.last_seen_at, None);
    assert_eq!(view.presence, Presence::Unknown);
    assert_eq!(view.session, None);
    assert_eq!(view.standing(), PeerStanding::Unlinked(Presence::Unknown));
    assert!(!view.is_connected());
}

#[test]
fn a_peer_that_has_spoken_projects_the_instant_it_spoke() {
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    roster
        .record_discovery(test_peers::bob(), vec![endpoint()], T0)
        .unwrap();
    roster
        .record_heartbeat(test_peers::bob(), later(1_000))
        .unwrap();

    let view = view_of(&roster, test_peers::bob(), later(2_000));

    assert_eq!(view.last_seen_at, Some(later(1_000)));
    assert_eq!(view.presence, Presence::Online);
    assert_eq!(view.standing(), PeerStanding::Unlinked(Presence::Online));
}

#[test]
fn standing_is_a_function_of_the_fields_the_view_already_holds() {
    // No new data crosses the port for it: whatever a caller does to a view, its
    // standing follows the two fields it can already read, so there is nothing
    // for the standing to disagree with.
    for session in [
        None,
        Some(SessionState::Connecting),
        Some(SessionState::Established),
        Some(SessionState::Closed),
    ] {
        for presence in [
            Presence::Unknown,
            Presence::Online,
            Presence::Stale,
            Presence::Offline,
        ] {
            let view = KnownPeerView {
                peer: test_peers::bob(),
                endpoints: vec![endpoint()],
                presence,
                last_seen_at: None,
                session,
            };

            assert_eq!(
                view.standing(),
                PeerStanding::of(session, presence),
                "session {session:?}, presence {presence}"
            );
            assert_eq!(
                view.is_connected(),
                view.standing().is_linked(),
                "session {session:?}, presence {presence}"
            );
        }
    }
}

// ------------------------------------------------------- the observed screen

#[test]
fn a_counted_peer_that_is_not_answering_is_not_a_bare_absence_word() {
    // The screenshot, reproduced: two established sessions whose evidence has
    // aged past the offline window. `connected (2 peers)` was true and
    // `offline` was true, and nothing tied them together.
    //
    // The count is deliberately unchanged — suppressing it would hide two
    // working links (S4). What changed is that both readings now come from one
    // classification, so those rows are `Linked(Offline)`: a value distinct
    // from the `Unlinked(Offline)` of a peer with no link at all, which is what
    // lets the renderer say "connected, not answering" instead of "offline".
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());
    for peer in [test_peers::bob(), test_peers::carol()] {
        roster.record_discovery(peer, vec![endpoint()], T0).unwrap();
        roster
            .open_session(peer, SessionDirection::Outbound, T0)
            .unwrap();
        roster.establish_session(peer, T0).unwrap();
    }
    // A peer with no link that has also gone quiet: the row that looks the same
    // today and must not.
    roster
        .record_discovery(test_peers::dave(), vec![endpoint()], T0)
        .unwrap();
    roster.record_heartbeat(test_peers::dave(), T0).unwrap();

    let now = later(WINDOWS.offline().as_millis() + 1);
    let standings: Vec<PeerStanding> = roster
        .known_peers()
        .map(|entry| KnownPeerView::of(entry, now, WINDOWS).standing())
        .collect();

    assert_eq!(
        NetworkStatus::from_standings(&standings),
        NetworkStatus::from_connected_peers(2),
        "the two links are real and stay counted"
    );
    assert_eq!(
        view_of(&roster, test_peers::bob(), now).standing(),
        PeerStanding::Linked(Presence::Offline)
    );
    assert_eq!(
        view_of(&roster, test_peers::dave(), now).standing(),
        PeerStanding::Unlinked(Presence::Offline)
    );
    assert_ne!(
        view_of(&roster, test_peers::bob(), now).standing(),
        view_of(&roster, test_peers::dave(), now).standing(),
        "a link that is up to a silent peer is not the same state as no link"
    );
}

// ------------------------------------------------------ the coherence property

/// A deterministic pseudo-random source of roster shapes.
///
/// The property has to hold for *arbitrary* rosters, and a handful of
/// hand-built ones proves far less — the defect survived a suite of hand-built
/// rosters. This is an xorshift64 over an explicit seed: no RNG crate, no
/// entropy, and a failing case is reproducible from the seed printed in the
/// assertion.
struct Shapes(u64);

impl Shapes {
    const fn seeded(seed: u64) -> Self {
        // Any non-zero state will do; xorshift is stuck at zero.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    /// An instant in the first four minutes of the timeline — long enough,
    /// against 30s/60s windows, for evidence to be fresh, stale, or long gone
    /// depending on when the reading is taken.
    fn instant(&mut self) -> Millis {
        later(self.below(240_000))
    }
}

/// A roster of `size` peers, each put into one of the shapes the roster API can
/// actually produce.
///
/// The shapes are the real transitions rather than hand-set fields: a test that
/// assembles states the aggregate cannot reach would prove the property over a
/// space the program never visits.
fn arbitrary_roster(seed: u64, size: usize) -> PeerRoster {
    let mut shapes = Shapes::seeded(seed);
    let mut roster = PeerRoster::for_local_peer(test_peers::alice());

    for peer in test_peers::synthetic(size) {
        let recorded_at = shapes.instant();
        roster
            .record_discovery(peer, vec![endpoint()], recorded_at)
            .expect("a synthetic peer is never the local peer");

        match shapes.below(6) {
            // Told about, never heard from, never dialled.
            0 => {}
            // Heard from over somebody else's link: evidence, but nothing to
            // send over.
            1 => {
                roster.record_heartbeat(peer, shapes.instant()).unwrap();
            }
            // Dialling out: a live session that is not yet a link, and our own
            // dial is no evidence about them.
            2 => {
                roster
                    .open_session(peer, SessionDirection::Outbound, shapes.instant())
                    .unwrap();
            }
            // Being dialled: still connecting, but an inbound open is evidence.
            3 => {
                roster
                    .open_session(peer, SessionDirection::Inbound, shapes.instant())
                    .unwrap();
            }
            // Established: the handshake completed, which is both a link and
            // evidence.
            4 => {
                let at = shapes.instant();
                roster
                    .open_session(peer, SessionDirection::Outbound, at)
                    .unwrap();
                roster.establish_session(peer, at).unwrap();
            }
            // Established and then closed: evidence survives, the link does
            // not.
            _ => {
                let at = shapes.instant();
                roster
                    .open_session(peer, SessionDirection::Outbound, at)
                    .unwrap();
                roster.establish_session(peer, at).unwrap();
                roster.close_session(peer).unwrap();
            }
        }
    }

    roster
}

#[test]
fn the_status_count_and_the_roster_standings_never_disagree() {
    // The test that would have caught the observed screen.
    //
    // For any roster and any instant, the number in the status line and the
    // number of rows that show as linked are the same number — not because two
    // computations were checked against each other, but because both are read
    // off one slice of standings. The third assertion pins that to the
    // aggregate's own session predicate, so the derivation cannot drift away
    // from what `Isolated` has always meant (D4, D5).
    let mut seen: HashSet<(bool, Presence)> = HashSet::new();

    for seed in 0..64u64 {
        for size in 0..=8usize {
            let roster = arbitrary_roster(seed, size);
            let mut readings = Shapes::seeded(seed ^ 0xA5A5_A5A5);

            for _ in 0..4 {
                let now = readings.instant();
                let views: Vec<KnownPeerView> = roster
                    .known_peers()
                    .map(|entry| KnownPeerView::of(entry, now, WINDOWS))
                    .collect();
                let standings: Vec<PeerStanding> =
                    views.iter().map(KnownPeerView::standing).collect();

                let status = NetworkStatus::from_standings(&standings);
                let linked = standings
                    .iter()
                    .filter(|standing| standing.is_linked())
                    .count();
                let where_ = format!("seed {seed}, size {size}, now {now}");

                assert_eq!(
                    status.connected_peers(),
                    linked,
                    "status line and rows disagree: {where_}"
                );
                assert_eq!(
                    status,
                    NetworkStatus::from_connected_peers(roster.established_session_count()),
                    "the one derivation drifted from the session predicate: {where_}"
                );
                assert_eq!(views.len(), roster.len(), "every peer has a row: {where_}");

                for (view, standing) in views.iter().zip(&standings) {
                    assert_eq!(
                        view.is_connected(),
                        standing.is_linked(),
                        "row and count predicate disagree for {:?}: {where_}",
                        view.peer
                    );
                    assert_eq!(
                        standing.presence(),
                        view.presence,
                        "standing dropped the presence for {:?}: {where_}",
                        view.peer
                    );
                    // A counted peer is never the same value as an uncounted
                    // one with the same presence, so no counted peer can render
                    // as a bare absence word (S4).
                    if standing.is_linked() {
                        assert_ne!(
                            *standing,
                            PeerStanding::Unlinked(standing.presence()),
                            "a counted peer collapsed into an unlinked row: {where_}"
                        );
                    }

                    seen.insert((standing.is_linked(), view.presence));
                }
            }
        }
    }

    // Guards the generator, not the rule: a property over rosters that never
    // contain a linked-but-silent peer would pass while proving nothing about
    // the case that was reported.
    for required in [
        (true, Presence::Online),
        (true, Presence::Stale),
        (true, Presence::Offline),
        (false, Presence::Unknown),
        (false, Presence::Online),
        (false, Presence::Stale),
        (false, Presence::Offline),
    ] {
        assert!(
            seen.contains(&required),
            "the generated rosters never produced linked={} with {}",
            required.0,
            required.1
        );
    }
}

#[test]
fn an_established_session_is_the_only_thing_that_links_a_peer() {
    // Pins the predicate to the aggregate's, entry by entry, over the same
    // arbitrary rosters: `KnownPeer::is_connected` and the view's standing are
    // two readings of one fact and must never diverge.
    for seed in 0..32u64 {
        let roster = arbitrary_roster(seed, 8);
        let now = Shapes::seeded(seed ^ 0x5A5A_5A5A).instant();

        for entry in roster.known_peers() {
            let view = KnownPeerView::of(entry, now, WINDOWS);

            assert_eq!(
                view.standing().is_linked(),
                KnownPeer::is_connected(entry),
                "seed {seed}, peer {:?}",
                entry.peer()
            );
        }
    }
}
