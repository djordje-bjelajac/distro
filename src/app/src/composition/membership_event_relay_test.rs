use membership::domain::events::MembershipEvent;
use membership::ports::EventPublisherPort;
use shared_types::{PeerConnected, PeerDisconnected};

use crate::composition::MembershipEventRelay;
use crate::test_peers::{alice, bob};

fn connected(peer: shared_types::PeerId) -> MembershipEvent {
    MembershipEvent::PeerConnected(PeerConnected { peer })
}

fn disconnected(peer: shared_types::PeerId) -> MembershipEvent {
    MembershipEvent::PeerDisconnected(PeerDisconnected { peer })
}

#[test]
fn published_events_come_back_in_order() {
    // The port requires it: a disconnect that overtook its connect would leave
    // `messaging` believing a dead peer is live.
    let relay = MembershipEventRelay::new();

    relay
        .publish(connected(alice()))
        .expect("the queue accepts");
    relay.publish(connected(bob())).expect("the queue accepts");
    relay
        .publish(disconnected(alice()))
        .expect("the queue accepts");

    assert_eq!(
        relay.drain(),
        vec![connected(alice()), connected(bob()), disconnected(alice())]
    );
}

#[test]
fn draining_empties_the_queue() {
    let relay = MembershipEventRelay::new();
    relay
        .publish(connected(alice()))
        .expect("the queue accepts");

    assert_eq!(relay.drain().len(), 1);
    assert!(relay.drain().is_empty());
}

#[test]
fn overflow_drops_the_oldest_and_counts_it() {
    // The newest disconnect is the one a pending direct message is waiting on
    // (D10); a stale connect nobody consumed is worth less.
    let relay = MembershipEventRelay::with_capacity(2);

    relay
        .publish(connected(alice()))
        .expect("the queue accepts");
    relay.publish(connected(bob())).expect("the queue accepts");
    relay
        .publish(disconnected(bob()))
        .expect("the queue accepts");

    assert_eq!(relay.drain(), vec![connected(bob()), disconnected(bob())]);
    assert_eq!(relay.dropped(), 1);
}

#[test]
fn nothing_is_dropped_while_the_queue_has_room() {
    let relay = MembershipEventRelay::new();

    for _ in 0..MembershipEventRelay::DEFAULT_CAPACITY {
        relay
            .publish(connected(alice()))
            .expect("the queue accepts");
    }

    assert_eq!(relay.dropped(), 0);
}

#[test]
fn context_internal_events_are_queued_too() {
    // The relay does not filter: `is_cross_context` is the consumer's
    // decision, and the UI wants the discovery and join events as well.
    use membership::domain::Millis;
    use membership::domain::events::PeerDiscovered;

    let relay = MembershipEventRelay::new();
    let event = MembershipEvent::PeerDiscovered(PeerDiscovered {
        peer: alice(),
        at: Millis::from_millis(7),
    });

    relay.publish(event).expect("the queue accepts");

    assert_eq!(relay.drain(), vec![event]);
}
