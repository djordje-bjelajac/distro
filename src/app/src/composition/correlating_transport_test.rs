use std::sync::{Arc, Mutex};

use messaging::domain::{ConversationId, MessageBody, MessageId, Millis, SequenceNumber};
use messaging::ports::{
    MessagePayload, MessageTransportError, MessageTransportPort, UnsignedEnvelope,
};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::composition::{CorrelatingTransport, DeliveryIndex};
use crate::test_peers::{alice, bob};

#[derive(Default)]
struct RecordingTransport {
    directs: Mutex<Vec<(PeerId, Envelope)>>,
    broadcasts: Mutex<Vec<Envelope>>,
    refuses: Mutex<bool>,
}

impl MessageTransportPort for RecordingTransport {
    fn send_direct(&self, to: PeerId, envelope: &Envelope) -> Result<(), MessageTransportError> {
        if *self.refuses.lock().expect("no panic") {
            return Err(MessageTransportError::PeerUnreachable);
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

/// An envelope shaped exactly as `messaging`'s outbound composer shapes one.
fn envelope(author: PeerId, kind: PayloadKind, sequence: u64, seed: u8) -> Envelope {
    let payload = MessagePayload::new(
        SequenceNumber::new(sequence).expect("a non-zero sequence"),
        Millis::from_millis(1_000),
        MessageBody::new("hello").expect("an admissible body"),
    );

    UnsignedEnvelope::draft(author, ProtocolVersion::CURRENT, kind, payload.encode())
        .into_signed(EnvelopeSignature::new([seed; EnvelopeSignature::LENGTH]))
}

fn wired() -> (
    Arc<RecordingTransport>,
    Arc<DeliveryIndex>,
    CorrelatingTransport,
) {
    let inner = Arc::new(RecordingTransport::default());
    let index = Arc::new(DeliveryIndex::new());
    let transport = CorrelatingTransport::new(Arc::clone(&inner) as Arc<_>, Arc::clone(&index));

    (inner, index, transport)
}

#[test]
fn a_direct_send_is_correlated_to_the_message_it_carries() {
    // The one moment a signature and a `MessageId` exist in the same place.
    let (_inner, index, transport) = wired();
    let envelope = envelope(alice(), PayloadKind::DirectMessage, 7, 3);

    transport
        .send_direct(bob(), &envelope)
        .expect("the fake transport accepts");

    assert_eq!(
        index.take(&envelope.signature),
        Some(MessageId::new(
            alice(),
            ConversationId::Direct(bob()),
            SequenceNumber::new(7).expect("a non-zero sequence")
        ))
    );
}

#[test]
fn the_envelope_still_reaches_the_transport_unchanged() {
    let (inner, _index, transport) = wired();
    let envelope = envelope(alice(), PayloadKind::DirectMessage, 1, 4);

    transport
        .send_direct(bob(), &envelope)
        .expect("the fake transport accepts");

    assert_eq!(
        *inner.directs.lock().expect("no panic"),
        vec![(bob(), envelope)]
    );
}

#[test]
fn a_refused_send_is_still_correlated() {
    // The refusal is turned into `Failed` by the send path; the index entry
    // costs nothing and is evicted in time. Recording only on success would
    // lose the acknowledgement of a send whose reply raced the return.
    let (inner, index, transport) = wired();
    *inner.refuses.lock().expect("no panic") = true;
    let envelope = envelope(alice(), PayloadKind::DirectMessage, 2, 5);

    let sent = transport.send_direct(bob(), &envelope);

    assert_eq!(sent, Err(MessageTransportError::PeerUnreachable));
    assert!(index.take(&envelope.signature).is_some());
}

#[test]
fn a_broadcast_is_not_correlated() {
    // Gossip has no recipient and no acknowledgement (D3, AC10), so there is
    // nothing to correlate and an entry would only wait to be evicted.
    let (inner, index, transport) = wired();
    let envelope = envelope(alice(), PayloadKind::BroadcastMessage, 1, 6);

    transport
        .publish_broadcast(&envelope)
        .expect("the fake transport accepts");

    assert_eq!(index.outstanding(), 0);
    assert_eq!(inner.broadcasts.lock().expect("no panic").len(), 1);
}

#[test]
fn an_unreadable_payload_is_sent_anyway_and_simply_not_correlated() {
    // A correlation problem must not become a delivery failure: the envelope
    // is well-formed and signed, and the recipient may well read it.
    let (inner, index, transport) = wired();
    let envelope = UnsignedEnvelope::draft(
        alice(),
        ProtocolVersion::CURRENT,
        PayloadKind::DirectMessage,
        vec![0x01, 0x02],
    )
    .into_signed(EnvelopeSignature::new([7; EnvelopeSignature::LENGTH]));

    transport
        .send_direct(bob(), &envelope)
        .expect("the fake transport accepts");

    assert_eq!(index.outstanding(), 0);
    assert_eq!(inner.directs.lock().expect("no panic").len(), 1);
}
