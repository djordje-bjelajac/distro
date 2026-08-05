use shared_types::{PayloadKind, ProtocolVersion};

use crate::domain::DeliveryFailure;
use crate::ports::port_fakes::{
    CheckingVerifier, FailingTransport, RecordingSigner, RecordingTransport,
};
use crate::ports::{
    EnvelopeSignerPort, EnvelopeVerifierPort, MessageTransportError, MessageTransportPort,
    SignatureVerdict, UnsignedEnvelope,
};
use crate::test_peers;

fn draft(kind: PayloadKind) -> UnsignedEnvelope {
    UnsignedEnvelope::draft(
        test_peers::alice(),
        ProtocolVersion::CURRENT,
        kind,
        b"hello".to_vec(),
    )
}

#[test]
fn the_port_is_object_safe_so_one_transport_can_be_shared() {
    let transport = RecordingTransport::default();
    let port: &dyn MessageTransportPort = &transport;
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(draft(PayloadKind::DirectMessage))
        .expect("signed");

    assert!(port.send_direct(test_peers::bob(), &envelope).is_ok());
}

#[test]
fn a_direct_message_is_addressed_by_peer_identity_alone() {
    // Canvas §4 hard rule: this context never learns what an endpoint is. The
    // whole address is a `PeerId`; who can reach it, and over what path, is
    // `membership`'s business.
    let transport = RecordingTransport::default();
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(draft(PayloadKind::DirectMessage))
        .expect("signed");

    transport
        .send_direct(test_peers::bob(), &envelope)
        .expect("the fake transport accepts");

    assert_eq!(transport.sent_direct(), [(test_peers::bob(), envelope)]);
    assert!(transport.published().is_empty());
}

#[test]
fn a_broadcast_names_no_recipient_at_all() {
    let transport = RecordingTransport::default();
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(draft(PayloadKind::BroadcastMessage))
        .expect("signed");

    transport
        .publish_broadcast(&envelope)
        .expect("the fake transport accepts");

    assert_eq!(transport.published(), [envelope]);
    assert!(transport.sent_direct().is_empty());
}

#[test]
fn signing_then_sending_delivers_bytes_that_still_verify() {
    // The round trip the whole context rests on: what the signer signed is
    // what the transport carried, and a receiver verifies it against the
    // envelope's own author (invariant 4, AC6).
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let transport = RecordingTransport::default();
    let draft = draft(PayloadKind::DirectMessage);
    let signed_input = draft.signable_bytes();

    let envelope = signer.seal(draft).expect("the local key is available");
    transport
        .send_direct(test_peers::bob(), &envelope)
        .expect("the fake transport accepts");

    let (recipient, carried) = transport.sent_direct().remove(0);
    assert_eq!(recipient, test_peers::bob());
    assert_eq!(
        carried.signable_bytes(),
        signed_input,
        "the transport carried exactly the bytes that were signed"
    );
    assert_eq!(signer.signed_inputs(), [signed_input]);
    assert_eq!(
        CheckingVerifier.verify(&carried),
        Ok(SignatureVerdict::Valid)
    );
}

#[test]
fn a_message_altered_in_flight_no_longer_verifies() {
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let transport = RecordingTransport::default();
    let envelope = signer
        .seal(draft(PayloadKind::DirectMessage))
        .expect("signed");
    transport
        .send_direct(test_peers::bob(), &envelope)
        .expect("accepted");

    let (_, mut carried) = transport.sent_direct().remove(0);
    carried.payload = b"hello?".to_vec();

    assert_eq!(
        CheckingVerifier.verify(&carried),
        Ok(SignatureVerdict::Invalid)
    );
}

// -------------------------------------------------- failures the user sees

#[test]
fn every_transport_failure_maps_to_a_delivery_failure_the_user_can_act_on() {
    // AC11/D10: silent loss is not a state, so every way a send can fail has
    // to arrive at a reason a 1:1 message can display.
    let table = [
        (
            MessageTransportError::Unavailable,
            DeliveryFailure::TransportUnavailable,
        ),
        (
            MessageTransportError::PeerUnreachable,
            DeliveryFailure::PeerUnreachable,
        ),
        (
            MessageTransportError::NoRelayAvailable,
            DeliveryFailure::NoRelayAvailable,
        ),
        (
            MessageTransportError::SessionClosed,
            DeliveryFailure::SessionClosed,
        ),
        (
            MessageTransportError::NotAcknowledged,
            DeliveryFailure::RetriesExhausted,
        ),
    ];

    for (error, expected) in table {
        assert_eq!(error.as_delivery_failure(), expected);
    }
}

#[test]
fn a_failing_transport_reports_its_typed_error() {
    let transport = FailingTransport(MessageTransportError::NoRelayAvailable);
    let signer = RecordingSigner::holding_key_of(test_peers::alice());
    let envelope = signer
        .seal(draft(PayloadKind::DirectMessage))
        .expect("signed");

    assert_eq!(
        transport.send_direct(test_peers::bob(), &envelope),
        Err(MessageTransportError::NoRelayAvailable)
    );
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        MessageTransportError::NoRelayAvailable.to_string(),
        "no peer was available to relay to the recipient"
    );
    assert_eq!(
        MessageTransportError::Unavailable.to_string(),
        "the message transport is not available"
    );
}
