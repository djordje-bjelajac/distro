use std::sync::{Arc, Mutex};

use membership::domain::Millis;
use membership::domain::events::{
    MembershipEvent, NetworkJoined, PeerDiscovered, PeerPresenceExpired,
};
use messaging::domain::events::MessageDeliveryStateChanged;
use messaging::ports::{MessagingCommandError, PeerLifecyclePort};
use shared_types::{PeerConnected, PeerDisconnected, PeerId};

use crate::composition::{Diagnostics, NoticeFeed};
use crate::runtime::LifecycleFanout;
use crate::test_peers::{alice, bob};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Connected(PeerId),
    Disconnected(PeerId),
}

#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<Call>>,
    refuses: Mutex<bool>,
}

impl PeerLifecyclePort for Recorder {
    fn peer_connected(&self, event: PeerConnected) -> Result<(), MessagingCommandError> {
        self.calls
            .lock()
            .expect("no panic")
            .push(Call::Connected(event.peer));

        if *self.refuses.lock().expect("no panic") {
            return Err(MessagingCommandError::Publisher(
                messaging::ports::EventPublisherError::Unavailable,
            ));
        }
        Ok(())
    }

    fn peer_disconnected(
        &self,
        event: PeerDisconnected,
    ) -> Result<Vec<MessageDeliveryStateChanged>, MessagingCommandError> {
        self.calls
            .lock()
            .expect("no panic")
            .push(Call::Disconnected(event.peer));

        if *self.refuses.lock().expect("no panic") {
            return Err(MessagingCommandError::Publisher(
                messaging::ports::EventPublisherError::Unavailable,
            ));
        }
        Ok(Vec::new())
    }
}

fn wired() -> (
    Arc<Recorder>,
    Arc<Diagnostics>,
    Arc<NoticeFeed>,
    LifecycleFanout,
) {
    let recorder = Arc::new(Recorder::default());
    let diagnostics = Arc::new(Diagnostics::default());
    let notices = Arc::new(NoticeFeed::new());
    let fanout = LifecycleFanout::new(
        Arc::clone(&recorder) as Arc<_>,
        Arc::clone(&diagnostics),
        Arc::clone(&notices),
    );

    (recorder, diagnostics, notices, fanout)
}

fn calls(recorder: &Recorder) -> Vec<Call> {
    recorder.calls.lock().expect("no panic").clone()
}

#[test]
fn a_connected_peer_reaches_messaging() {
    // Without it a conversation is never rehydrated from the counter, and a
    // restarted peer goes permanently mute (D12, AC16).
    let (recorder, _diagnostics, _notices, fanout) = wired();

    fanout.fan(&MembershipEvent::PeerConnected(PeerConnected {
        peer: bob(),
    }));

    assert_eq!(calls(&recorder), vec![Call::Connected(bob())]);
}

#[test]
fn a_disconnected_peer_reaches_messaging() {
    // Without it a pending direct stays pending forever, which AC11 calls
    // silent loss wearing a spinner (D10).
    let (recorder, _diagnostics, _notices, fanout) = wired();

    fanout.fan(&MembershipEvent::PeerDisconnected(PeerDisconnected {
        peer: bob(),
    }));

    assert_eq!(calls(&recorder), vec![Call::Disconnected(bob())]);
}

#[test]
fn context_internal_events_never_cross() {
    // `messaging` must never learn what an endpoint, a session, or a presence
    // is (canvas §4).
    let (recorder, _diagnostics, _notices, fanout) = wired();

    fanout.fan(&MembershipEvent::PeerDiscovered(PeerDiscovered {
        peer: alice(),
        at: Millis::from_millis(1),
    }));
    fanout.fan(&MembershipEvent::PeerPresenceExpired(PeerPresenceExpired {
        peer: alice(),
        last_evidence_at: Millis::from_millis(1),
        at: Millis::from_millis(2),
    }));
    fanout.fan(&MembershipEvent::NetworkJoined(NetworkJoined {
        at: Millis::from_millis(1),
        connected_peers: std::num::NonZeroUsize::new(1).expect("one"),
    }));

    assert!(calls(&recorder).is_empty());
}

#[test]
fn order_is_preserved_across_a_connect_and_its_disconnect() {
    let (recorder, _diagnostics, _notices, fanout) = wired();

    fanout.fan(&MembershipEvent::PeerConnected(PeerConnected {
        peer: bob(),
    }));
    fanout.fan(&MembershipEvent::PeerDisconnected(PeerDisconnected {
        peer: bob(),
    }));

    assert_eq!(
        calls(&recorder),
        vec![Call::Connected(bob()), Call::Disconnected(bob())]
    );
}

#[test]
fn a_refusal_is_counted_and_stated_rather_than_propagated() {
    // A fan-out that returned `Err` would stop the drain and lose every event
    // behind it.
    let (recorder, diagnostics, notices, fanout) = wired();
    *recorder.refuses.lock().expect("no panic") = true;

    fanout.fan(&MembershipEvent::PeerDisconnected(PeerDisconnected {
        peer: bob(),
    }));

    assert_eq!(diagnostics.port_refusals(), 1);
    assert_eq!(notices.all().len(), 1);
}
