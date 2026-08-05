use std::num::NonZeroUsize;

use crate::domain::NetworkStatus;

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
