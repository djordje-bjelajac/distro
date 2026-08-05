use std::sync::{Arc, Mutex};

use messaging::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, MessageTransportError, MessageTransportPort,
    UnsignedEnvelope,
};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::composition::{HeartbeatBeacon, HeartbeatError};
use crate::test_peers::{alice, bob};

/// Signs anything authored by the peer it speaks for, and nothing else — the
/// production signer's rule, without a key.
struct FakeSigner {
    peer: PeerId,
    fails: bool,
}

impl EnvelopeSignerPort for FakeSigner {
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        if self.fails {
            return Err(EnvelopeSignerError::SigningFailed);
        }
        if unsigned.author() != self.peer {
            return Err(EnvelopeSignerError::AuthorMismatch);
        }

        Ok(EnvelopeSignature::new([42; EnvelopeSignature::LENGTH]))
    }
}

#[derive(Default)]
struct RecordingTransport {
    directs: Mutex<Vec<(PeerId, Envelope)>>,
    broadcasts: Mutex<Vec<Envelope>>,
    refuses: Mutex<bool>,
}

impl MessageTransportPort for RecordingTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        self.directs
            .lock()
            .expect("no panic")
            .push((to, envelope.clone()));
        Ok(())
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        if *self.refuses.lock().expect("no panic") {
            return Err(MessageTransportError::Unavailable);
        }
        self.broadcasts
            .lock()
            .expect("no panic")
            .push(envelope.clone());
        Ok(())
    }
}

fn wired(fails_to_sign: bool) -> (Arc<RecordingTransport>, HeartbeatBeacon) {
    let transport = Arc::new(RecordingTransport::default());
    let beacon = HeartbeatBeacon::new(
        alice(),
        ProtocolVersion::CURRENT,
        Arc::new(FakeSigner {
            peer: alice(),
            fails: fails_to_sign,
        }) as Arc<_>,
        Arc::clone(&transport) as Arc<_>,
    );

    (transport, beacon)
}

#[test]
fn a_heartbeat_goes_out_on_the_broadcast_topic() {
    let (transport, beacon) = wired(false);

    beacon.emit().expect("the fake collaborators accept");

    let broadcasts = transport.broadcasts.lock().expect("no panic");
    assert_eq!(broadcasts.len(), 1);
    assert!(transport.directs.lock().expect("no panic").is_empty());
}

#[test]
fn a_heartbeat_is_authored_by_this_peer_and_carries_the_heartbeat_kind() {
    let (transport, beacon) = wired(false);

    beacon.emit().expect("the fake collaborators accept");

    let broadcasts = transport.broadcasts.lock().expect("no panic");
    let envelope = broadcasts.first().expect("one heartbeat");
    assert_eq!(envelope.author, alice());
    assert_ne!(envelope.author, bob());
    assert_eq!(envelope.kind, PayloadKind::Heartbeat);
    assert_eq!(envelope.version, ProtocolVersion::CURRENT);
}

#[test]
fn a_heartbeat_carries_no_payload() {
    // A timestamp inside it would be the author's claim about their own clock,
    // which no rule in either context may read.
    let (transport, beacon) = wired(false);

    beacon.emit().expect("the fake collaborators accept");

    let broadcasts = transport.broadcasts.lock().expect("no panic");
    assert!(
        broadcasts
            .first()
            .expect("one heartbeat")
            .payload
            .is_empty()
    );
}

#[test]
fn a_heartbeat_is_signed() {
    // An unsigned one would be a free way to assert a peer's presence.
    let (transport, beacon) = wired(false);

    beacon.emit().expect("the fake collaborators accept");

    let broadcasts = transport.broadcasts.lock().expect("no panic");
    assert_eq!(
        broadcasts.first().expect("one heartbeat").signature,
        EnvelopeSignature::new([42; EnvelopeSignature::LENGTH])
    );
}

#[test]
fn a_signer_refusal_is_reported_and_nothing_is_published() {
    let (transport, beacon) = wired(true);

    let emitted = beacon.emit();

    assert_eq!(
        emitted,
        Err(HeartbeatError::Signer(EnvelopeSignerError::SigningFailed))
    );
    assert!(transport.broadcasts.lock().expect("no panic").is_empty());
}

#[test]
fn a_transport_refusal_is_reported() {
    let (transport, beacon) = wired(false);
    *transport.refuses.lock().expect("no panic") = true;

    assert_eq!(
        beacon.emit(),
        Err(HeartbeatError::Transport(
            MessageTransportError::Unavailable
        ))
    );
}
