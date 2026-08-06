use std::num::NonZeroUsize;

use crate::domain::{NetworkStatus, PeerStanding, Presence};

#[test]
fn no_connected_peers_is_isolated_which_is_a_normal_state() {
    let status = NetworkStatus::from_connected_peers(0);

    assert_eq!(status, NetworkStatus::Isolated);
    assert!(status.is_isolated());
    assert_eq!(status.connected_peers(), 0);
}

#[test]
fn connected_carries_the_peer_count() {
    let status = NetworkStatus::from_connected_peers(3);

    assert_eq!(
        status,
        NetworkStatus::Connected(NonZeroUsize::new(3).unwrap())
    );
    assert_eq!(status.connected_peers(), 3);
    assert!(!status.is_isolated());
}

#[test]
fn joining_is_neither_isolated_nor_connected() {
    let status = NetworkStatus::Joining;

    assert!(!status.is_isolated());
    assert_eq!(status.connected_peers(), 0);
    assert_ne!(status, NetworkStatus::Isolated);
}

// ------------------------------------------------------- one derivation (D5)

#[test]
fn from_standings_counts_exactly_the_linked_standings() {
    let standings = [
        PeerStanding::Linked(Presence::Online),
        PeerStanding::Unlinked(Presence::Online),
        PeerStanding::Linked(Presence::Offline),
        PeerStanding::Unlinked(Presence::Unknown),
        PeerStanding::Linked(Presence::Stale),
        PeerStanding::Unlinked(Presence::Offline),
    ];

    let status = NetworkStatus::from_standings(&standings);

    assert_eq!(status, NetworkStatus::from_connected_peers(3));
    assert_eq!(status.connected_peers(), 3);
}

#[test]
fn no_linked_standing_is_isolated_however_live_the_roster_looks() {
    // Live by evidence and reachable are different claims. A peer whose
    // heartbeat arrived over somebody else's link cannot be sent a direct
    // message, so counting it would say `connected (3)` where zero directs can
    // be sent (D4).
    let standings = [
        PeerStanding::Unlinked(Presence::Online),
        PeerStanding::Unlinked(Presence::Online),
        PeerStanding::Unlinked(Presence::Online),
    ];

    assert_eq!(
        NetworkStatus::from_standings(&standings),
        NetworkStatus::Isolated
    );
}

#[test]
fn an_empty_roster_is_isolated() {
    assert_eq!(NetworkStatus::from_standings(&[]), NetworkStatus::Isolated);
}

#[test]
fn a_link_to_a_peer_that_is_not_answering_still_counts() {
    // S4. The honest report is `connected (1 peer)` with that row saying the
    // link is up and nothing is coming back. Suppressing the count to make the
    // screen agree with itself would hide a working link — one of the two ways
    // the naive reading of "make them agree" is achievable only by lying.
    let standings = [PeerStanding::Linked(Presence::Offline)];

    assert_eq!(
        NetworkStatus::from_standings(&standings),
        NetworkStatus::from_connected_peers(1)
    );
    assert_eq!(
        NetworkStatus::from_standings(&standings).to_string(),
        "connected (1 peer)"
    );
}

#[test]
fn a_link_to_a_peer_never_heard_from_still_counts() {
    // The other absence word. A handshake completed and no evidence has aged
    // yet; the link is real and `Unknown` says only that nothing has come back
    // over it so far.
    let standings = [PeerStanding::Linked(Presence::Unknown)];

    assert_eq!(
        NetworkStatus::from_standings(&standings),
        NetworkStatus::from_connected_peers(1)
    );
}

#[test]
fn from_standings_is_the_same_arithmetic_as_the_count_it_shares() {
    // The two entry points must never drift apart: `from_connected_peers` is
    // what the session count feeds, and any difference between them would be a
    // second predicate — exactly what D5 removes.
    let every_standing = [
        PeerStanding::Linked(Presence::Unknown),
        PeerStanding::Linked(Presence::Online),
        PeerStanding::Linked(Presence::Stale),
        PeerStanding::Linked(Presence::Offline),
        PeerStanding::Unlinked(Presence::Unknown),
        PeerStanding::Unlinked(Presence::Online),
        PeerStanding::Unlinked(Presence::Stale),
        PeerStanding::Unlinked(Presence::Offline),
    ];

    // Every subset of the eight distinct standings, so no combination of
    // presences and link states is left untried.
    for subset in 0u32..(1 << every_standing.len()) {
        let standings: Vec<PeerStanding> = every_standing
            .iter()
            .enumerate()
            .filter(|(index, _)| subset & (1 << index) != 0)
            .map(|(_, standing)| *standing)
            .collect();
        let linked = standings.iter().filter(|s| s.is_linked()).count();

        assert_eq!(
            NetworkStatus::from_standings(&standings),
            NetworkStatus::from_connected_peers(linked),
            "subset {subset:#010b}"
        );
        assert_eq!(
            NetworkStatus::from_standings(&standings).connected_peers(),
            linked,
            "subset {subset:#010b}"
        );
    }
}

#[test]
fn displays_a_status_line() {
    assert_eq!(NetworkStatus::Isolated.to_string(), "isolated");
    assert_eq!(NetworkStatus::Joining.to_string(), "joining");
    assert_eq!(
        NetworkStatus::from_connected_peers(1).to_string(),
        "connected (1 peer)"
    );
    assert_eq!(
        NetworkStatus::from_connected_peers(4).to_string(),
        "connected (4 peers)"
    );
}
