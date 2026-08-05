use std::num::NonZeroUsize;

use membership::domain::NetworkStatus;

use crate::test_peers::alice;
use crate::tui::{PeerLabels, StatusLine};

fn line(status: NetworkStatus) -> StatusLine {
    StatusLine::build(status, alice(), "peer-d75a9801", "broadcast")
}

#[test]
fn the_three_network_states_all_read_differently() {
    // The canvas asks for exactly `Isolated | Joining | Connected(n)`.
    let isolated = line(NetworkStatus::Isolated).network;
    let joining = line(NetworkStatus::Joining).network;
    let connected = line(NetworkStatus::Connected(
        NonZeroUsize::new(3).expect("three"),
    ))
    .network;

    assert_eq!(isolated, "isolated");
    assert_eq!(joining, "joining");
    assert!(connected.contains('3'), "{connected}");
    assert_ne!(isolated, joining);
    assert_ne!(joining, connected);
}

#[test]
fn one_connected_peer_reads_in_the_singular() {
    let connected = line(NetworkStatus::Connected(NonZeroUsize::new(1).expect("one"))).network;

    assert_eq!(connected, "connected (1 peer)");
}

#[test]
fn isolation_is_stated_plainly_and_never_as_a_failure() {
    // `Isolated` is a normal state (canvas §2.2, S7): a fresh install on a
    // quiet network with no ticket is supposed to reach it.
    let isolated = line(NetworkStatus::Isolated);

    assert!(isolated.is_isolated());
    let text = isolated.text().to_lowercase();
    assert!(!text.contains("error"), "{text}");
    assert!(!text.contains("fail"), "{text}");
}

#[test]
fn the_line_names_this_peer_by_display_name_and_fingerprint() {
    let line = line(NetworkStatus::Isolated);

    assert!(line.identity.contains("peer-d75a9801"));
    assert!(line.identity.contains(&PeerLabels::short(alice())));
}

#[test]
fn the_line_names_the_conversation_on_screen() {
    let line = StatusLine::build(
        NetworkStatus::Isolated,
        alice(),
        "peer-x",
        "direct 21fe 31df",
    );

    assert!(line.text().contains("direct 21fe 31df"));
}

#[test]
fn a_connected_status_is_not_isolated() {
    let connected = line(NetworkStatus::Connected(NonZeroUsize::new(2).expect("two")));

    assert!(!connected.is_isolated());
}
