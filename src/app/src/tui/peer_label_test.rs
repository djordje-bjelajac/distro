use crate::test_peers::{alice, bob};
use crate::tui::PeerLabels;

#[test]
fn the_local_peer_is_called_you() {
    // Reading your own fingerprint back at yourself teaches nothing.
    let labels = PeerLabels::for_local(alice());

    assert_eq!(labels.label(alice()), "you");
    assert!(labels.is_local(alice()));
}

#[test]
fn a_remote_peer_is_labelled_by_its_fingerprint() {
    let labels = PeerLabels::for_local(alice());

    assert_eq!(labels.label(bob()), PeerLabels::short(bob()));
    assert!(!labels.is_local(bob()));
}

#[test]
fn two_peers_get_two_labels() {
    assert_ne!(PeerLabels::short(alice()), PeerLabels::short(bob()));
}

#[test]
fn a_label_is_a_prefix_of_the_full_fingerprint() {
    // The short form is for a column; the full digest is what a user compares
    // out of band before verifying (AC6).
    let full = PeerLabels::full_fingerprint(bob());

    assert!(full.starts_with(&PeerLabels::short(bob())));
    assert!(full.len() > PeerLabels::short(bob()).len());
}

#[test]
fn a_label_is_stable_across_calls() {
    // A roster whose labels changed between frames would be unreadable.
    assert_eq!(PeerLabels::short(bob()), PeerLabels::short(bob()));
}

#[test]
fn a_label_carries_no_control_characters() {
    // Labels are echoed into a terminal, where an escape sequence could forge
    // interface structure. A fingerprint is hex and spaces by construction.
    let label = PeerLabels::full_fingerprint(bob());

    assert!(!label.chars().any(char::is_control));
    assert!(label.chars().all(|c| c.is_ascii_hexdigit() || c == ' '));
}
