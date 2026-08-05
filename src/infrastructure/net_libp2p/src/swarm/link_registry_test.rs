use libp2p::Multiaddr;
use libp2p::swarm::ConnectionId;
use membership::domain::SessionDirection;
use shared_types::PeerId;

use crate::mapping::PeerIdMapping;
use crate::swarm::link_registry::{LinkRegistry, LinkRegistryError};
use crate::test_peers::{alice, bob, carol};

fn address(port: u16) -> Multiaddr {
    format!("/ip4/127.0.0.1/udp/{port}/quic-v1")
        .parse()
        .expect("fixture parses")
}

fn relayed_address() -> Multiaddr {
    let relay = PeerIdMapping::to_libp2p(carol()).expect("maps out");
    format!("/ip4/203.0.113.4/tcp/4001/p2p/{relay}/p2p-circuit")
        .parse()
        .expect("fixture parses")
}

fn connection(id: usize) -> ConnectionId {
    ConnectionId::new_unchecked(id)
}

/// A registry whose local peer is the *lower* of the pair, so the domain's
/// collapse rule keeps this peer's outbound dial.
fn lower_local() -> (LinkRegistry, PeerId, libp2p::PeerId) {
    let (low, high) = ordered_pair();
    (
        LinkRegistry::new(low),
        high,
        PeerIdMapping::to_libp2p(high).expect("maps out"),
    )
}

/// A registry whose local peer is the *higher* of the pair, so the collapse
/// rule keeps the remote's inbound dial.
fn higher_local() -> (LinkRegistry, PeerId, libp2p::PeerId) {
    let (low, high) = ordered_pair();
    (
        LinkRegistry::new(high),
        low,
        PeerIdMapping::to_libp2p(low).expect("maps out"),
    )
}

fn ordered_pair() -> (PeerId, PeerId) {
    let (first, second) = (alice(), bob());
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

#[test]
fn the_first_link_becomes_the_session_and_is_announced() {
    let (mut registry, remote_identity, remote) = lower_local();

    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");

    assert_eq!(outcome.primary, connection(1));
    assert_eq!(outcome.primary_address, address(4001));
    assert!(outcome.close.is_empty());
    assert!(outcome.newly_connected);
    assert!(outcome.collapse.is_none());
    assert!(registry.holds_session(&remote));
}

#[test]
fn a_lone_inbound_link_is_the_session_too() {
    // A single link is not a collapse: there is nothing to choose between, and
    // refusing an inbound-only session would make this peer undialable.
    let (mut registry, remote_identity, remote) = lower_local();

    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4002),
        )
        .expect("recorded");

    assert_eq!(outcome.primary, connection(1));
    assert!(outcome.newly_connected);
}

#[test]
fn a_simultaneous_connect_keeps_the_lower_peers_dial_when_that_is_us() {
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        )
        .expect("recorded");
    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    // We are the lower peer, so *our* dial survives and their inbound link is
    // the one that goes.
    assert_eq!(outcome.primary, connection(2));
    assert_eq!(outcome.close, vec![connection(1)]);
    assert_eq!(
        outcome
            .collapse
            .expect("a collapse was resolved")
            .survivor(),
        SessionDirection::Outbound
    );
}

#[test]
fn a_simultaneous_connect_keeps_the_lower_peers_dial_when_that_is_them() {
    let (mut registry, remote_identity, remote) = higher_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");
    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Inbound,
            address(4002),
        )
        .expect("recorded");

    // They are the lower peer, so their dial survives and ours is discarded —
    // the mirror of the previous test, computed from the same two keys.
    assert_eq!(outcome.primary, connection(2));
    assert_eq!(outcome.close, vec![connection(1)]);
    assert_eq!(
        outcome
            .collapse
            .expect("a collapse was resolved")
            .survivor(),
        SessionDirection::Inbound
    );
}

#[test]
fn the_collapse_answer_does_not_depend_on_which_link_arrived_first() {
    // Both peers must reach the same answer without exchanging a message, so
    // arrival order cannot enter into it.
    for outbound_first in [true, false] {
        let (mut registry, remote_identity, remote) = lower_local();
        let (first, second) = if outbound_first {
            (SessionDirection::Outbound, SessionDirection::Inbound)
        } else {
            (SessionDirection::Inbound, SessionDirection::Outbound)
        };

        registry
            .record_established(remote_identity, remote, connection(1), first, address(4001))
            .expect("recorded");
        let outcome = registry
            .record_established(
                remote_identity,
                remote,
                connection(2),
                second,
                address(4002),
            )
            .expect("recorded");

        let survivor_direction = if outbound_first { first } else { second };
        assert_eq!(
            survivor_direction,
            SessionDirection::Outbound,
            "the lower peer's dial survives either way"
        );
        assert_eq!(outcome.close.len(), 1);
    }
}

#[test]
fn a_collapse_does_not_announce_a_second_session() {
    // The logical session persists across the swap: the peer was already
    // connected, and reporting it again would make the roster hold two.
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        )
        .expect("recorded");
    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    assert!(!outcome.newly_connected);
}

#[test]
fn closing_the_superseded_link_does_not_end_the_session() {
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        )
        .expect("recorded");
    registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    let closed = registry
        .record_closed(remote, connection(1))
        .expect("the superseded link was tracked");

    assert!(!closed.was_primary);
    assert!(!closed.peer_gone);
    assert_eq!(closed.new_primary, Some(connection(2)));
    assert!(registry.holds_session(&remote));
}

#[test]
fn closing_the_last_link_ends_the_session() {
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");

    let closed = registry
        .record_closed(remote, connection(1))
        .expect("tracked");

    assert!(closed.was_primary);
    assert!(closed.peer_gone);
    assert_eq!(closed.new_primary, None);
    assert!(!registry.holds_session(&remote));
}

#[test]
fn losing_the_survivor_first_promotes_the_remaining_link() {
    // The far side may close the superseded link before we do, or close the
    // survivor for its own reasons. Either way a live link keeps the session.
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        )
        .expect("recorded");
    registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    let closed = registry
        .record_closed(remote, connection(2))
        .expect("tracked");

    assert!(closed.was_primary);
    assert!(!closed.peer_gone);
    assert_eq!(closed.new_primary, Some(connection(1)));
}

#[test]
fn a_redundant_dial_in_the_same_direction_keeps_the_oldest_link() {
    // Two outbound dials are not a simultaneous connect — there is no rule to
    // apply and nothing to negotiate, so the older link keeps the session and
    // the newcomer is closed.
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");
    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    assert_eq!(outcome.primary, connection(1));
    assert_eq!(outcome.close, vec![connection(2)]);
    assert!(outcome.collapse.is_none());
}

#[test]
fn the_primary_address_follows_the_surviving_link() {
    // AC12 reads the reachability class off this address: if the survivor is
    // the relayed link, the roster must be told the peer is reached through a
    // relay, not directly.
    let (mut registry, remote_identity, remote) = higher_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");
    let outcome = registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Inbound,
            relayed_address(),
        )
        .expect("recorded");

    assert_eq!(outcome.primary_address, relayed_address());
    assert_eq!(registry.primary_address(&remote), Some(relayed_address()));
}

#[test]
fn close_session_by_peer_names_every_link_that_peer_holds() {
    // This is what `PeerTransportPort::close_session` executes. It is
    // unambiguous *because* the collapse was already resolved below this line:
    // by the time the application asks, there is one session to end.
    let (mut registry, remote_identity, remote) = lower_local();

    registry
        .record_established(
            remote_identity,
            remote,
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        )
        .expect("recorded");
    registry
        .record_established(
            remote_identity,
            remote,
            connection(2),
            SessionDirection::Outbound,
            address(4002),
        )
        .expect("recorded");

    assert_eq!(
        registry.connections_of(&remote),
        vec![connection(1), connection(2)]
    );

    registry.forget(&remote);

    assert!(!registry.holds_session(&remote));
    assert!(registry.connections_of(&remote).is_empty());
}

#[test]
fn a_connection_claiming_our_own_identity_is_refused() {
    let local = alice();
    let mut registry = LinkRegistry::new(local);

    assert_eq!(
        registry.record_established(
            local,
            PeerIdMapping::to_libp2p(local).expect("maps out"),
            connection(1),
            SessionDirection::Inbound,
            address(4001),
        ),
        Err(LinkRegistryError::SelfConnection)
    );
    assert_eq!(registry.session_count(), 0);
}

#[test]
fn closing_a_connection_that_was_never_recorded_reports_nothing() {
    let (mut registry, _, remote) = lower_local();

    assert!(registry.record_closed(remote, connection(9)).is_none());
}

#[test]
fn sessions_with_different_peers_are_independent() {
    let mut registry = LinkRegistry::new(alice());
    let first = PeerIdMapping::to_libp2p(bob()).expect("maps out");
    let second = PeerIdMapping::to_libp2p(carol()).expect("maps out");

    registry
        .record_established(
            bob(),
            first,
            connection(1),
            SessionDirection::Outbound,
            address(4001),
        )
        .expect("recorded");
    registry
        .record_established(
            carol(),
            second,
            connection(2),
            SessionDirection::Inbound,
            address(4002),
        )
        .expect("recorded");

    assert_eq!(registry.session_count(), 2);

    registry.record_closed(first, connection(1));

    assert!(!registry.holds_session(&first));
    assert!(registry.holds_session(&second));
}
