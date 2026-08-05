use futures::io::Cursor;
use libp2p::request_response::Codec;

use crate::swarm::direct_message_codec::{DirectMessageAck, DirectMessageCodec};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

#[test]
fn round_trips_a_request_frame() {
    let mut codec = DirectMessageCodec::new(1_024);
    let payload = b"an envelope this codec never reads".to_vec();

    let mut wire = Vec::new();
    block_on(codec.write_request(&DirectMessageCodec::PROTOCOL, &mut wire, payload.clone()))
        .expect("writes");

    let mut reader = Cursor::new(wire);
    let read =
        block_on(codec.read_request(&DirectMessageCodec::PROTOCOL, &mut reader)).expect("reads");

    assert_eq!(read, payload);
}

#[test]
fn round_trips_an_empty_frame() {
    let mut codec = DirectMessageCodec::new(1_024);

    let mut wire = Vec::new();
    block_on(codec.write_request(&DirectMessageCodec::PROTOCOL, &mut wire, Vec::new()))
        .expect("writes");

    let mut reader = Cursor::new(wire);
    assert_eq!(
        block_on(codec.read_request(&DirectMessageCodec::PROTOCOL, &mut reader)).expect("reads"),
        Vec::<u8>::new()
    );
}

#[test]
fn round_trips_both_acknowledgement_states() {
    let mut codec = DirectMessageCodec::new(1_024);

    for ack in [DirectMessageAck::Accepted, DirectMessageAck::Refused] {
        let mut wire = Vec::new();
        block_on(codec.write_response(&DirectMessageCodec::PROTOCOL, &mut wire, ack))
            .expect("writes");

        let mut reader = Cursor::new(wire);
        assert_eq!(
            block_on(codec.read_response(&DirectMessageCodec::PROTOCOL, &mut reader))
                .expect("reads"),
            ack
        );
    }
}

#[test]
fn an_unknown_acknowledgement_code_reads_as_a_refusal() {
    // A future peer that invents a new refusal reason must not have it read as
    // success — AC11 makes a silently-lost message a bug, not a state.
    let mut codec = DirectMessageCodec::new(1_024);
    let mut reader = Cursor::new(vec![200_u8]);

    assert_eq!(
        block_on(codec.read_response(&DirectMessageCodec::PROTOCOL, &mut reader)).expect("reads"),
        DirectMessageAck::Refused
    );
}

#[test]
fn refuses_an_oversize_frame_from_its_length_prefix_alone() {
    // The whole point of hand-writing this codec: the body is never read, and
    // nothing is allocated for a claim of 4 GiB.
    let mut codec = DirectMessageCodec::new(64);
    let mut wire = u32::MAX.to_be_bytes().to_vec();
    wire.extend_from_slice(b"only a few bytes actually follow");

    let mut reader = Cursor::new(wire);
    let error = block_on(codec.read_request(&DirectMessageCodec::PROTOCOL, &mut reader))
        .expect_err("refused");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        error.to_string().contains("over the 64-byte cap"),
        "the refusal must state the cap it hit: {error}"
    );
}

#[test]
fn a_frame_exactly_at_the_cap_is_accepted() {
    let mut codec = DirectMessageCodec::new(16);
    let payload = vec![7_u8; 16];

    let mut wire = Vec::new();
    block_on(codec.write_request(&DirectMessageCodec::PROTOCOL, &mut wire, payload.clone()))
        .expect("writes");

    let mut reader = Cursor::new(wire);
    assert_eq!(
        block_on(codec.read_request(&DirectMessageCodec::PROTOCOL, &mut reader)).expect("reads"),
        payload
    );
}

#[test]
fn refuses_to_write_a_frame_over_the_cap() {
    let mut codec = DirectMessageCodec::new(8);
    let mut wire = Vec::new();

    assert!(
        block_on(codec.write_request(&DirectMessageCodec::PROTOCOL, &mut wire, vec![0_u8; 9]))
            .is_err()
    );
}

#[test]
fn refuses_a_truncated_frame_without_panicking() {
    let mut codec = DirectMessageCodec::new(1_024);
    let mut wire = Vec::new();
    block_on(codec.write_request(
        &DirectMessageCodec::PROTOCOL,
        &mut wire,
        b"a complete frame".to_vec(),
    ))
    .expect("writes");

    for length in 0..wire.len() {
        let mut reader = Cursor::new(wire[..length].to_vec());
        assert!(
            block_on(codec.read_request(&DirectMessageCodec::PROTOCOL, &mut reader)).is_err(),
            "a frame truncated to {length} bytes must be refused"
        );
    }
}

#[test]
fn the_protocol_name_is_versioned() {
    // A framing change must be a protocol older peers do not negotiate, not a
    // frame they misread.
    assert_eq!(
        DirectMessageCodec::PROTOCOL.as_ref(),
        "/distro/direct/1.0.0"
    );
}
