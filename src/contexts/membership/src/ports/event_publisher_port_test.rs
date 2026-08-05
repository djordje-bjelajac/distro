use std::num::NonZeroUsize;

use shared_types::{PeerConnected, PeerDisconnected};

use crate::domain::Millis;
use crate::domain::events::{MembershipEvent, NetworkJoined, PeerDiscovered};
use crate::ports::port_fakes::{FailingPublisher, RecordingPublisher};
use crate::ports::{EventPublisherError, EventPublisherPort};
use crate::test_peers;

const AT: Millis = Millis::from_millis(2_000);

#[test]
fn publishes_this_contexts_own_events() {
    let publisher = RecordingPublisher::new();
    let port: &dyn EventPublisherPort = &publisher;
    let discovered = PeerDiscovered {
        peer: test_peers::bob(),
        at: AT,
    };

    assert_eq!(port.publish(MembershipEvent::from(discovered)), Ok(()));

    assert_eq!(
        publisher.published(),
        vec![MembershipEvent::from(discovered)]
    );
}

#[test]
fn publishes_the_cross_context_events_through_the_same_port() {
    // Canvas §2.2†: `membership` is the only publisher of PeerConnected and
    // PeerDisconnected, and it publishes them exactly as `shared_types`
    // defines them — no second, membership-flavoured copy exists.
    let publisher = RecordingPublisher::new();
    let port: &dyn EventPublisherPort = &publisher;

    port.publish(MembershipEvent::from(PeerConnected {
        peer: test_peers::bob(),
    }))
    .unwrap();
    port.publish(MembershipEvent::from(PeerDisconnected {
        peer: test_peers::bob(),
    }))
    .unwrap();

    let published = publisher.published();
    assert_eq!(published.len(), 2);
    assert!(published.iter().all(MembershipEvent::is_cross_context));
}

#[test]
fn events_are_published_in_the_order_they_are_handed_over() {
    // Order carries meaning: a PeerDisconnected that overtook its
    // PeerConnected would leave `messaging` believing a dead peer is live.
    let publisher = RecordingPublisher::new();
    let port: &dyn EventPublisherPort = &publisher;
    let joined = MembershipEvent::from(NetworkJoined {
        at: AT,
        connected_peers: NonZeroUsize::new(1).unwrap(),
    });
    let connected = MembershipEvent::from(PeerConnected {
        peer: test_peers::bob(),
    });

    port.publish(connected).unwrap();
    port.publish(joined).unwrap();

    assert_eq!(publisher.published(), vec![connected, joined]);
}

#[test]
fn a_failing_publisher_reports_a_typed_error() {
    let publisher = FailingPublisher(EventPublisherError::Unavailable);
    let port: &dyn EventPublisherPort = &publisher;

    assert_eq!(
        port.publish(MembershipEvent::from(PeerConnected {
            peer: test_peers::bob()
        })),
        Err(EventPublisherError::Unavailable)
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let error = EventPublisherError::Unavailable;

    assert_eq!(error.to_string(), "the event publisher is not available");
    let _: &dyn std::error::Error = &error;
}
