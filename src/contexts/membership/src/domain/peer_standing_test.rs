use crate::domain::{PeerStanding, Presence, SessionState};

/// Every combination of session state and presence a roster entry can hold,
/// with the standing it must classify to.
///
/// Written out by hand rather than computed: a table generated from the same
/// predicate [`PeerStanding::of`] uses would prove only that the rule equals
/// itself. The twelve rows the canvas enumerates are the `None` /
/// `Connecting` / `Established` blocks; the four [`SessionState::Closed`] rows
/// are here because the classification must be *total* over the type, not over
/// the states a test author happened to think of.
const TRUTH_TABLE: [(Option<SessionState>, Presence, PeerStanding); 16] = [
    // No session at all: known because something named this peer, or dialled
    // once and let go. Nothing can be sent to it now.
    (
        None,
        Presence::Unknown,
        PeerStanding::Unlinked(Presence::Unknown),
    ),
    (
        None,
        Presence::Online,
        PeerStanding::Unlinked(Presence::Online),
    ),
    (
        None,
        Presence::Stale,
        PeerStanding::Unlinked(Presence::Stale),
    ),
    (
        None,
        Presence::Offline,
        PeerStanding::Unlinked(Presence::Offline),
    ),
    // Dialling: a commitment to one link, but not yet a usable one. A direct
    // message sent now has nowhere to go, so this is not a link.
    (
        Some(SessionState::Connecting),
        Presence::Unknown,
        PeerStanding::Unlinked(Presence::Unknown),
    ),
    (
        Some(SessionState::Connecting),
        Presence::Online,
        PeerStanding::Unlinked(Presence::Online),
    ),
    (
        Some(SessionState::Connecting),
        Presence::Stale,
        PeerStanding::Unlinked(Presence::Stale),
    ),
    (
        Some(SessionState::Connecting),
        Presence::Offline,
        PeerStanding::Unlinked(Presence::Offline),
    ),
    // Established: the handshake completed, so this peer is reachable right
    // now — whatever the evidence says about whether it is answering.
    (
        Some(SessionState::Established),
        Presence::Unknown,
        PeerStanding::Linked(Presence::Unknown),
    ),
    (
        Some(SessionState::Established),
        Presence::Online,
        PeerStanding::Linked(Presence::Online),
    ),
    (
        Some(SessionState::Established),
        Presence::Stale,
        PeerStanding::Linked(Presence::Stale),
    ),
    (
        Some(SessionState::Established),
        Presence::Offline,
        PeerStanding::Linked(Presence::Offline),
    ),
    // Closed is terminal: the roster clears the slot, but the classification
    // must still answer rather than fall through a wildcard.
    (
        Some(SessionState::Closed),
        Presence::Unknown,
        PeerStanding::Unlinked(Presence::Unknown),
    ),
    (
        Some(SessionState::Closed),
        Presence::Online,
        PeerStanding::Unlinked(Presence::Online),
    ),
    (
        Some(SessionState::Closed),
        Presence::Stale,
        PeerStanding::Unlinked(Presence::Stale),
    ),
    (
        Some(SessionState::Closed),
        Presence::Offline,
        PeerStanding::Unlinked(Presence::Offline),
    ),
];

const EVERY_SESSION: [Option<SessionState>; 4] = [
    None,
    Some(SessionState::Connecting),
    Some(SessionState::Established),
    Some(SessionState::Closed),
];

const EVERY_PRESENCE: [Presence; 4] = [
    Presence::Unknown,
    Presence::Online,
    Presence::Stale,
    Presence::Offline,
];

#[test]
fn the_truth_table_covers_every_combination_exactly_once() {
    // Guards the table itself: "total, with no ambiguous row" is a claim about
    // the *table*, and a missing or duplicated row would quietly weaken every
    // assertion made from it.
    for session in EVERY_SESSION {
        for presence in EVERY_PRESENCE {
            let rows = TRUTH_TABLE
                .iter()
                .filter(|(row_session, row_presence, _)| {
                    *row_session == session && *row_presence == presence
                })
                .count();

            assert_eq!(rows, 1, "session {session:?} with presence {presence}");
        }
    }
}

#[test]
fn classifies_every_session_and_presence_combination() {
    for (session, presence, expected) in TRUTH_TABLE {
        assert_eq!(
            PeerStanding::of(session, presence),
            expected,
            "session {session:?} with presence {presence}"
        );
    }
}

#[test]
fn only_an_established_session_links_a_peer() {
    // The predicate must be the one `PeerRoster::established_session_count`
    // already uses. `Connecting` is the trap: it is a live session, so a
    // predicate written as "has a live session" would count a dial in flight
    // and put a peer in `connected (n)` that cannot yet be sent anything.
    for presence in EVERY_PRESENCE {
        assert!(
            PeerStanding::of(Some(SessionState::Established), presence).is_linked(),
            "established, {presence}"
        );

        for session in [
            None,
            Some(SessionState::Connecting),
            Some(SessionState::Closed),
        ] {
            assert!(
                !PeerStanding::of(session, presence).is_linked(),
                "{session:?}, {presence}"
            );
        }
    }
}

#[test]
fn linked_offline_is_a_state_of_its_own_not_a_contradiction() {
    // The link is up and the peer is not answering: both halves are
    // independently true, and the pair is the most informative thing this
    // context can say about a peer whose process died with its socket open.
    // Making it unrepresentable would mean either dropping it from the count
    // (hiding a working link) or asserting `Online` from the link (fabricating
    // evidence) — canvas D5, S4.
    let linked = PeerStanding::Linked(Presence::Offline);
    let unlinked = PeerStanding::Unlinked(Presence::Offline);

    assert_ne!(linked, unlinked);
    assert!(linked.is_linked());
    assert!(!unlinked.is_linked());
    assert_eq!(linked.presence(), Presence::Offline);
    assert_eq!(unlinked.presence(), Presence::Offline);
}

#[test]
fn no_presence_collapses_the_two_standings_together() {
    // The distinction has to survive for *every* presence, not just the one in
    // the screenshot: a renderer keyed on presence alone is exactly the defect,
    // and `Unknown` is the other absence word the roster shows.
    for presence in EVERY_PRESENCE {
        assert_ne!(
            PeerStanding::Linked(presence),
            PeerStanding::Unlinked(presence),
            "presence {presence}"
        );
    }
}

#[test]
fn a_standing_carries_the_presence_it_was_derived_from() {
    // The row still reports what the evidence says; the standing adds the link
    // to it rather than replacing it. A standing that dropped the presence
    // would force the renderer back to a second lookup — and a second lookup is
    // where the two stories diverged.
    for session in EVERY_SESSION {
        for presence in EVERY_PRESENCE {
            assert_eq!(
                PeerStanding::of(session, presence).presence(),
                presence,
                "session {session:?} with presence {presence}"
            );
        }
    }
}
