use crate::ports::AuthorPolicyPort;
use crate::ports::port_fakes::LocalBlockList;
use crate::test_peers;

#[test]
fn the_port_is_object_safe_so_one_policy_can_be_shared() {
    let policy = LocalBlockList::blocking([test_peers::carol()]);
    let port: &dyn AuthorPolicyPort = &policy;

    assert!(port.is_blocked(test_peers::carol()));
    assert!(!port.is_blocked(test_peers::bob()));
}

#[test]
fn an_empty_policy_blocks_nobody() {
    let policy = LocalBlockList::default();

    for peer in [test_peers::bob(), test_peers::carol(), test_peers::dave()] {
        assert!(!policy.is_blocked(peer));
    }
}

#[test]
fn blocking_one_peer_says_nothing_about_any_other() {
    // Invariant 11: blocking is purely local and entirely per-peer. Nothing is
    // announced, and no peer's state is inferred from another's.
    let policy = LocalBlockList::blocking([test_peers::bob(), test_peers::dave()]);

    assert!(policy.is_blocked(test_peers::bob()));
    assert!(policy.is_blocked(test_peers::dave()));
    assert!(!policy.is_blocked(test_peers::carol()));
}
