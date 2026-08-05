use std::num::NonZeroUsize;

use shared_types::{PeerConnected, PeerDisconnected};

use crate::domain::Millis;
use crate::domain::events::{
    MembershipEvent, NetworkJoined, NetworkLeft, PeerDiscovered, PeerPresenceExpired,
};
use crate::test_peers;

const AT: Millis = Millis::from_millis(4_000);

#[test]
fn every_published_event_of_this_context_has_a_variant() {
    // One union so `EventPublisherPort` can stay object-safe with a single
    // method, and so a new event cannot be added without every publisher
    // handling it.
    let events = [
        MembershipEvent::from(NetworkJoined {
            at: AT,
            connected_peers: NonZeroUsize::new(2).unwrap(),
        }),
        MembershipEvent::from(NetworkLeft { at: AT }),
        MembershipEvent::from(PeerDiscovered {
            peer: test_peers::bob(),
            at: AT,
        }),
        MembershipEvent::from(PeerPresenceExpired {
            peer: test_peers::bob(),
            last_evidence_at: Millis::ZERO,
            at: AT,
        }),
        MembershipEvent::from(PeerConnected {
            peer: test_peers::bob(),
        }),
        MembershipEvent::from(PeerDisconnected {
            peer: test_peers::bob(),
        }),
    ];

    assert_eq!(events.len(), 6);
}

#[test]
fn the_cross_context_events_keep_their_shared_types_payload() {
    // Canvas §2.2†: PeerConnected/PeerDisconnected travel between contexts and
    // carry a PeerId and nothing else, so no context learns another's
    // internals. Wrapping them here must not add anything.
    let wrapped = MembershipEvent::from(PeerConnected {
        peer: test_peers::bob(),
    });

    assert_eq!(
        wrapped,
        MembershipEvent::PeerConnected(PeerConnected {
            peer: test_peers::bob()
        })
    );
}

#[test]
fn a_context_internal_event_is_distinguishable_from_a_cross_context_one() {
    let internal = MembershipEvent::from(PeerDiscovered {
        peer: test_peers::bob(),
        at: AT,
    });
    let cross_context = MembershipEvent::from(PeerConnected {
        peer: test_peers::bob(),
    });

    assert!(!internal.is_cross_context());
    assert!(cross_context.is_cross_context());
    assert_ne!(internal, cross_context);
}

#[test]
fn presence_expiry_carries_the_evidence_it_was_derived_from() {
    // Invariant 7: the event reports a derivation, so it must show the inputs
    // rather than assert a fact about the remote peer.
    let event = PeerPresenceExpired {
        peer: test_peers::bob(),
        last_evidence_at: Millis::from_millis(1_000),
        at: AT,
    };

    assert_eq!(event.peer, test_peers::bob());
    assert_eq!(event.last_evidence_at, Millis::from_millis(1_000));
    assert_eq!(event.at, AT);
}
