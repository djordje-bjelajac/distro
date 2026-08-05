use membership::domain::Millis;
use membership::domain::events::{MembershipEvent, PeerDiscovered};
use shared_types::{PeerConnected, PeerId};

use crate::crypto::SimKeypair;
use crate::fabric::{DropCause, FrameLabel};
use crate::trace::{EventTrace, PeerLifecycle, TraceEvent};

fn alice() -> PeerId {
    SimKeypair::derived(1, "alice").peer()
}

fn bob() -> PeerId {
    SimKeypair::derived(1, "bob").peer()
}

fn labelled() -> EventTrace {
    let trace = EventTrace::new();
    trace.label(alice(), "alice");
    trace.label(bob(), "bob");
    trace
}

#[test]
fn a_new_trace_is_empty() {
    let trace = EventTrace::new();

    assert!(trace.is_empty());
    assert_eq!(trace.render(), "");
}

#[test]
fn entries_keep_the_order_they_happened_in() {
    let trace = labelled();

    trace.record(
        1,
        TraceEvent::Lifecycle {
            peer: alice(),
            change: PeerLifecycle::Started,
        },
    );
    trace.record(
        2,
        TraceEvent::Lifecycle {
            peer: bob(),
            change: PeerLifecycle::Started,
        },
    );

    let ats: Vec<u64> = trace.entries().iter().map(|entry| entry.at).collect();
    assert_eq!(ats, vec![1, 2]);
}

#[test]
fn rendering_resolves_peers_to_their_labels() {
    let trace = labelled();
    trace.record(
        7,
        TraceEvent::FrameDelivered {
            from: alice(),
            to: bob(),
            frame: FrameLabel::Direct(3),
        },
    );

    assert_eq!(trace.render(), "         7 alice -> bob direct#3\n");
}

#[test]
fn an_unlabelled_peer_still_renders_deterministically() {
    // A missing label degrades readability, never reproducibility.
    let trace = EventTrace::new();
    trace.record(
        0,
        TraceEvent::Lifecycle {
            peer: alice(),
            change: PeerLifecycle::Stopped,
        },
    );

    let first = trace.render();
    assert!(first.contains("peer:"), "{first}");
    assert_eq!(first, trace.render());
}

#[test]
fn every_event_shape_renders_on_one_line() {
    let trace = labelled();

    trace.record(
        0,
        TraceEvent::FrameDropped {
            from: alice(),
            to: bob(),
            frame: FrameLabel::Broadcast(1),
            cause: DropCause::Partitioned,
        },
    );
    trace.record(
        1,
        TraceEvent::FrameRefused {
            from: alice(),
            to: bob(),
            frame: FrameLabel::SessionOpened,
            reason: "already open".to_owned(),
        },
    );
    trace.record(
        2,
        TraceEvent::PortRefused {
            peer: bob(),
            operation: "close-aged-gaps",
            reason: "the message log is not available".to_owned(),
        },
    );
    trace.record(
        3,
        TraceEvent::Membership {
            peer: alice(),
            event: MembershipEvent::PeerConnected(PeerConnected { peer: bob() }),
        },
    );

    let rendered = trace.render();

    assert_eq!(rendered.lines().count(), 4);
    assert!(rendered.contains("dropped(partitioned)"), "{rendered}");
    assert!(rendered.contains("refused(already open)"), "{rendered}");
    assert!(rendered.contains("refused close-aged-gaps"), "{rendered}");
    assert!(rendered.contains("peer-connected bob"), "{rendered}");
}

#[test]
fn events_can_be_read_back_per_peer() {
    let trace = labelled();
    let discovered = PeerDiscovered {
        peer: bob(),
        at: Millis::from_millis(5),
    };

    trace.record(
        5,
        TraceEvent::Membership {
            peer: alice(),
            event: MembershipEvent::PeerDiscovered(discovered),
        },
    );

    assert_eq!(
        trace.membership_events_of(alice()),
        vec![MembershipEvent::PeerDiscovered(discovered)]
    );
    assert!(trace.membership_events_of(bob()).is_empty());
    assert!(trace.messaging_events().is_empty());
}

#[test]
fn clearing_discards_entries_and_keeps_labels() {
    let trace = labelled();
    trace.record(
        1,
        TraceEvent::Lifecycle {
            peer: alice(),
            change: PeerLifecycle::Restarted,
        },
    );

    trace.clear();

    assert!(trace.is_empty());
    assert_eq!(trace.label_of(alice()), "alice");
}
