use crate::{PeerDisconnected, PeerId};

/// RFC 8032 §7.1 TEST 1 public key.
const RFC8032_TEST1_PUBLIC_KEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

#[test]
fn carries_only_the_peer_id() {
    let peer = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    let event = PeerDisconnected { peer };
    assert_eq!(event.peer, peer);
}

#[test]
fn equality_is_by_peer() {
    let peer = PeerId::from_public_key_bytes(RFC8032_TEST1_PUBLIC_KEY).unwrap();
    assert_eq!(PeerDisconnected { peer }, PeerDisconnected { peer });
}
