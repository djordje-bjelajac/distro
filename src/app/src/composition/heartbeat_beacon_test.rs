use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use messaging::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, MessageTransportError, MessageTransportPort,
    UnsignedEnvelope,
};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::composition::{HeartbeatBeacon, HeartbeatError, HeartbeatLedger, HeartbeatRound};
use crate::test_peers::{alice, bob, carol};

/// Signs anything authored by the peer it speaks for, and nothing else — the
/// production signer's rule, without a key.
struct FakeSigner {
    peer: PeerId,
    fails: bool,
    signings: AtomicU64,
}

impl EnvelopeSignerPort for FakeSigner {
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        self.signings.fetch_add(1, Ordering::Relaxed);

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
    /// Peers whose sends this transport will not take.
    refuses: Mutex<Vec<PeerId>>,
}

impl RecordingTransport {
    fn directs(&self) -> Vec<(PeerId, Envelope)> {
        self.directs.lock().expect("no panic").clone()
    }

    fn recipients(&self) -> Vec<PeerId> {
        self.directs()
            .into_iter()
            .map(|(peer, _)| peer)
            .collect::<Vec<_>>()
    }
}

impl MessageTransportPort for RecordingTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        if self.refuses.lock().expect("no panic").contains(&to) {
            return Err(MessageTransportError::Unavailable);
        }

        self.directs
            .lock()
            .expect("no panic")
            .push((to, envelope.clone()));
        Ok(())
    }

    fn publish_broadcast(&self, envelope: &Envelope) -> Result<(), MessageTransportError> {
        self.broadcasts
            .lock()
            .expect("no panic")
            .push(envelope.clone());
        Ok(())
    }
}

struct Wiring {
    transport: Arc<RecordingTransport>,
    signer: Arc<FakeSigner>,
    heartbeats: Arc<HeartbeatLedger>,
    beacon: HeartbeatBeacon,
}

fn wired(fails_to_sign: bool) -> Wiring {
    let transport = Arc::new(RecordingTransport::default());
    let signer = Arc::new(FakeSigner {
        peer: alice(),
        fails: fails_to_sign,
        signings: AtomicU64::new(0),
    });
    let heartbeats = Arc::new(HeartbeatLedger::new());

    let beacon = HeartbeatBeacon::new(
        alice(),
        ProtocolVersion::CURRENT,
        Arc::clone(&signer) as Arc<_>,
        Arc::clone(&transport) as Arc<_>,
        Arc::clone(&heartbeats),
    );

    Wiring {
        transport,
        signer,
        heartbeats,
        beacon,
    }
}

#[test]
fn one_heartbeat_goes_to_each_linked_peer() {
    let wiring = wired(false);

    let round = wiring
        .beacon
        .emit(&[bob(), carol()])
        .expect("the fake collaborators accept");

    assert_eq!(wiring.transport.recipients(), vec![bob(), carol()]);
    assert_eq!(
        round,
        HeartbeatRound {
            sent: 2,
            refused: 0
        }
    );
}

#[test]
fn a_peer_holding_no_session_is_not_sent_one() {
    // The caller decides who is linked; the beacon sends to exactly that set
    // and invents nobody. A peer left out of `linked` hears nothing at all —
    // including by way of a broadcast, which no longer exists (D7).
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob()])
        .expect("the fake collaborators accept");

    assert_eq!(wiring.transport.recipients(), vec![bob()]);
    assert!(
        !wiring.transport.recipients().contains(&carol()),
        "carol holds no session and must not be reachable by any other route"
    );
    assert!(
        wiring
            .transport
            .broadcasts
            .lock()
            .expect("no panic")
            .is_empty()
    );
}

#[test]
fn nothing_is_ever_published_on_the_broadcast_topic() {
    // Inverted from `a_heartbeat_goes_out_on_the_broadcast_topic`, which
    // asserted the mechanism D7 removes: liveness must not depend on
    // gossip-mesh formation, and one mechanism means the broadcast one is gone
    // rather than kept as a second path.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob(), carol()])
        .expect("the fake collaborators accept");

    assert!(
        wiring
            .transport
            .broadcasts
            .lock()
            .expect("no panic")
            .is_empty(),
        "a heartbeat on the topic is the defect, not a fallback"
    );
}

#[test]
fn no_linked_peers_sends_nothing_and_is_not_an_error() {
    // The ordinary state of an instance nobody has dialled yet. Reporting a
    // failure here would put a fault on screen for a fresh install behaving
    // exactly as designed.
    let wiring = wired(false);

    let round = wiring
        .beacon
        .emit(&[])
        .expect("nobody to send to is not a fault");

    assert_eq!(round, HeartbeatRound::default());
    assert!(wiring.transport.directs().is_empty());
    assert!(
        wiring
            .transport
            .broadcasts
            .lock()
            .expect("no panic")
            .is_empty()
    );
}

#[test]
fn nothing_is_signed_when_there_is_nobody_to_send_to() {
    // An Ed25519 operation every ten seconds for an envelope that goes nowhere,
    // and — worse — a signature recorded as outstanding that no report can ever
    // answer.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[])
        .expect("nobody to send to is not a fault");

    assert_eq!(wiring.signer.signings.load(Ordering::Relaxed), 0);
    assert_eq!(wiring.heartbeats.held(), 0);
}

#[test]
fn a_round_signs_once_however_many_peers_it_reaches() {
    // The envelope has no recipient field and no nonce, so a per-peer signature
    // would be the same bytes signed repeatedly for an identical result.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob(), carol()])
        .expect("the fake collaborators accept");

    assert_eq!(wiring.signer.signings.load(Ordering::Relaxed), 1);

    let directs = wiring.transport.directs();
    assert_eq!(directs[0].1, directs[1].1, "one envelope, sent twice");
}

#[test]
fn a_heartbeat_is_authored_by_this_peer_and_carries_the_heartbeat_kind() {
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob()])
        .expect("the fake collaborators accept");

    let directs = wiring.transport.directs();
    let (_, envelope) = directs.first().expect("one heartbeat");
    assert_eq!(envelope.author, alice());
    assert_ne!(envelope.author, bob());
    assert_eq!(envelope.kind, PayloadKind::Heartbeat);
    assert_eq!(envelope.version, ProtocolVersion::CURRENT);
}

#[test]
fn a_heartbeat_carries_no_payload() {
    // A timestamp inside it would be the author's claim about their own clock,
    // which no rule in either context may read.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob()])
        .expect("the fake collaborators accept");

    let directs = wiring.transport.directs();
    assert!(directs.first().expect("one heartbeat").1.payload.is_empty());
}

#[test]
fn a_heartbeat_is_signed() {
    // An unsigned one would be a free way to assert a peer's presence.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob()])
        .expect("the fake collaborators accept");

    let directs = wiring.transport.directs();
    assert_eq!(
        directs.first().expect("one heartbeat").1.signature,
        EnvelopeSignature::new([42; EnvelopeSignature::LENGTH])
    );
}

#[test]
fn the_signature_a_round_releases_is_recorded_as_a_heartbeats() {
    // S6: the transport answers for a heartbeat exactly as it answers for a
    // message, and this is the only thing that can tell the two reports apart.
    let wiring = wired(false);

    wiring
        .beacon
        .emit(&[bob()])
        .expect("the fake collaborators accept");

    assert!(
        wiring
            .heartbeats
            .is_heartbeat(&EnvelopeSignature::new([42; EnvelopeSignature::LENGTH]))
    );
}

#[test]
fn a_signer_refusal_fails_the_whole_round_and_nothing_is_sent() {
    let wiring = wired(true);

    let emitted = wiring.beacon.emit(&[bob(), carol()]);

    assert_eq!(
        emitted,
        Err(HeartbeatError::Signer(EnvelopeSignerError::SigningFailed))
    );
    assert!(wiring.transport.directs().is_empty());
    assert!(
        wiring
            .transport
            .broadcasts
            .lock()
            .expect("no panic")
            .is_empty()
    );
    assert_eq!(
        wiring.heartbeats.held(),
        0,
        "an envelope that was never signed has no signature to correlate"
    );
}

#[test]
fn a_transport_refusal_fails_one_peer_and_the_round_reaches_the_others() {
    // A refused send is news about one link. Abandoning the round would let a
    // peer whose link just dropped cost every other peer its evidence.
    let wiring = wired(false);
    *wiring.transport.refuses.lock().expect("no panic") = vec![bob()];

    let round = wiring
        .beacon
        .emit(&[bob(), carol()])
        .expect("a refused send is not a failed round");

    assert_eq!(
        round,
        HeartbeatRound {
            sent: 1,
            refused: 1
        }
    );
    assert_eq!(wiring.transport.recipients(), vec![carol()]);
}
