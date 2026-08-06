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
        // Kept consistent with the presence rather than always `Some`: a
        // never-heard-from peer is precisely one with no evidence instant, and
        // a fixture that carried one would be describing a state the roster
        // cannot produce (canvas D1).
        last_seen_at: if presence.is_unknown() {
            None
        } else {
            Some(Millis::from_millis(1))
        },
        session,
    }
}

/// Every standing a row can carry, so a rendering rule is stated about all of
/// them rather than about the two that were convenient.
const EVERY_STANDING: [(Option<SessionState>, Presence); 8] = [
    (None, Presence::Unknown),
    (None, Presence::Online),
    (None, Presence::Stale),
    (None, Presence::Offline),
    (Some(SessionState::Established), Presence::Unknown),
    (Some(SessionState::Established), Presence::Online),
    (Some(SessionState::Established), Presence::Stale),
    (Some(SessionState::Established), Presence::Offline),
];

fn row(session: Option<SessionState>, presence: Presence) -> crate::tui::RosterRow {
    roster_rows(
        &[view(bob(), presence, session, false)],
        PeerLabels::for_local(alice()),
        untrusted,
    )
    .remove(0)
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

    assert_eq!(rows[0].presence(), Presence::Offline);
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

    assert!(rows[0].presence().is_online() && !rows[0].connected());
    assert!(!rows[1].presence().is_online() && rows[1].connected());
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

    assert!(!rows[0].connected());
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

// --------------------------------------------------- the presence cell (OP-7)

#[test]
fn a_linked_offline_peer_and_an_unlinked_one_never_read_the_same() {
    // The regression guard for the observed screen: `connected (2 peers)` above
    // a roster in which every row read `offline`. `Linked(Offline)` is a
    // legitimate state — the session is up, the peer is not answering — and it
    // survives to the screen as a state of its own rather than being made
    // coherent by dropping it from the count or by claiming it is online
    // (canvas D5, safeguard S4).
    let linked = row(Some(SessionState::Established), Presence::Offline);
    let unlinked = row(None, Presence::Offline);

    assert_eq!(linked.presence_text(), "connected · not answering");
    assert_eq!(unlinked.presence_text(), "offline");
    assert_ne!(linked.presence_text(), unlinked.presence_text());
}

#[test]
fn a_never_heard_from_peer_renders_an_empty_presence_cell() {
    // After a cache load most rows are in this state, so a word here is printed
    // down the whole pane on every launch, and a column of `unknown` reads as a
    // fault. The blank is a rendering decision; the diagnostic label stays
    // `unknown` and is deliberately not what the row shows (canvas §3, D1).
    let unknown = row(None, Presence::Unknown);

    assert_eq!(unknown.presence_text(), "");
    assert_eq!(
        Presence::Unknown.to_string(),
        "unknown",
        "the diagnostic label is unchanged; only the rendering is blank"
    );
    assert_ne!(unknown.presence_text(), Presence::Unknown.to_string());
}

#[test]
fn a_never_heard_from_peer_is_shown_in_the_roster_rather_than_filtered_out() {
    // A peer we hold an address for and have never reached is a dialable
    // candidate. Hiding it turns "my peer vanished" into a support question
    // (canvas §3).
    let peers = vec![
        view(bob(), Presence::Unknown, None, false),
        view(carol(), Presence::Online, None, false),
    ];

    let rows = roster_rows(&peers, PeerLabels::for_local(alice()), untrusted);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].peer, bob());
    assert!(rows[0].presence().is_unknown());
    // Shown means shown: the row still carries what is actually known about the
    // peer, which is its identity and its badge — just no claim about presence.
    assert_eq!(rows[0].label, PeerLabels::short(bob()));
    assert_eq!(rows[0].trust_badge(), "?");
}

#[test]
fn no_linked_row_ever_renders_a_bare_absence_word() {
    // The A5 property at the level of one row: a peer the status line is
    // counting never states an absence on its own. The `Connected(n)`-wide form
    // of this is in `network_panes_test`.
    for (session, presence) in EVERY_STANDING {
        let row = row(session, presence);
        if !row.connected() {
            continue;
        }

        for absence in ["offline", "unknown", "gone", "absent", "disconnected"] {
            assert_ne!(
                row.presence_text(),
                absence,
                "a linked {presence} peer rendered the bare absence word {absence:?}"
            );
        }
    }
}

#[test]
fn the_words_shared_with_the_diagnostic_label_are_the_same_words() {
    // `presence_text` spells its own strings so the blank cell and
    // `connected · not answering` can exist at all. This pins the three it does
    // *not* change, so the row and the diagnostic cannot drift apart unnoticed.
    for presence in [Presence::Online, Presence::Stale, Presence::Offline] {
        assert_eq!(row(None, presence).presence_text(), presence.to_string());
    }
}

#[test]
fn every_standing_has_a_rendering_and_the_two_offline_ones_are_distinct() {
    // Exhaustive: eight standings, none left to a wildcard.
    let rendered: Vec<&str> = EVERY_STANDING
        .iter()
        .map(|(session, presence)| row(*session, *presence).presence_text())
        .collect();

    assert_eq!(
        rendered,
        vec![
            "",                          // Unlinked(Unknown)
            "online",                    // Unlinked(Online)
            "stale",                     // Unlinked(Stale)
            "offline",                   // Unlinked(Offline)
            "",                          // Linked(Unknown)
            "online",                    // Linked(Online)
            "stale",                     // Linked(Stale)
            "connected · not answering", // Linked(Offline)
        ]
    );
}
