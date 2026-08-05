use identity::domain::VerificationState;
use membership::domain::{Endpoint, Millis, Presence, SessionState};
use membership::ports::KnownPeerView;
use shared_types::PeerId;

use crate::composition::PeerTrust;
use crate::test_peers::{alice, bob, carol};
use crate::tui::{PeerLabels, roster_rows};

fn view(
    peer: PeerId,
    presence: Presence,
    session: Option<SessionState>,
    relayed: bool,
) -> KnownPeerView {
    KnownPeerView {
        peer,
        endpoints: vec![if relayed {
            Endpoint::relayed("/ip4/203.0.113.1/tcp/1/p2p-circuit").expect("a valid address")
        } else {
            Endpoint::direct("/ip4/10.0.0.1/tcp/1").expect("a valid address")
        }],
        presence,
        last_seen_at: Millis::from_millis(1),
        session,
    }
}

fn untrusted(_peer: PeerId) -> PeerTrust {
    PeerTrust::default()
}

#[test]
fn every_known_peer_gets_a_row_in_roster_order() {
    let peers = vec![
        view(bob(), Presence::Online, None, false),
        view(carol(), Presence::Offline, None, false),
    ];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].peer, bob());
    assert_eq!(rows[1].peer, carol());
}

#[test]
fn an_offline_peer_is_shown_rather_than_hidden() {
    // `Offline` is a derivation about evidence age, not a statement that a peer
    // is gone (invariant 7) — and the row is the only record it exists.
    let peers = vec![view(carol(), Presence::Offline, None, false)];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert_eq!(rows[0].presence, Presence::Offline);
}

#[test]
fn a_blocked_peer_is_shown_so_it_can_be_unblocked() {
    let peers = vec![view(bob(), Presence::Online, None, false)];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), |_| PeerTrust {
        verification: VerificationState::Unverified,
        blocked: true,
    });

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].trust_badge(), "⊘");
}

#[test]
fn online_and_connected_are_different_columns() {
    // A peer seen announcing itself a second ago is online with no session at
    // all; a peer holding a session goes stale if it stops speaking.
    let peers = vec![
        view(bob(), Presence::Online, None, false),
        view(
            carol(),
            Presence::Stale,
            Some(SessionState::Established),
            false,
        ),
    ];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert!(rows[0].presence.is_online() && !rows[0].connected);
    assert!(!rows[1].presence.is_online() && rows[1].connected);
}

#[test]
fn a_connecting_session_is_not_connected() {
    let peers = vec![view(
        bob(),
        Presence::Online,
        Some(SessionState::Connecting),
        false,
    )];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert!(!rows[0].connected);
    assert_eq!(rows[0].link_mark(), " ");
}

#[test]
fn a_relayed_link_is_marked_differently_from_a_direct_one() {
    // AC12: a relayed path depends on a stranger staying online, which is a
    // fact worth showing.
    let peers = vec![
        view(
            bob(),
            Presence::Online,
            Some(SessionState::Established),
            false,
        ),
        view(
            carol(),
            Presence::Online,
            Some(SessionState::Established),
            true,
        ),
    ];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert!(!rows[0].relayed);
    assert!(rows[1].relayed);
    assert_ne!(rows[0].link_mark(), rows[1].link_mark());
}

#[test]
fn the_four_trust_badges_are_all_distinct() {
    // The two axes are orthogonal, so a blocked *and* verified peer must not
    // read like either one alone.
    let peers = vec![view(bob(), Presence::Online, None, false)];
    let labels = PeerLabels::for_local(alice());

    let badge = |verification, blocked| {
        roster_rows(&peers, labels, |_| PeerTrust {
            verification,
            blocked,
        })[0]
            .trust_badge()
    };

    let badges = [
        badge(VerificationState::Unverified, false),
        badge(VerificationState::Verified, false),
        badge(VerificationState::Unverified, true),
        badge(VerificationState::Verified, true),
    ];

    let mut unique = badges.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4, "{badges:?}");
}

#[test]
fn a_peer_is_labelled_by_fingerprint_and_never_by_a_chosen_name() {
    let peers = vec![view(bob(), Presence::Online, None, false)];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert_eq!(rows[0].label, PeerLabels::short(bob()));
}
