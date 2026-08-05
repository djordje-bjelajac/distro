use libp2p::identity::{Keypair, PublicKey, ed25519};
use libp2p::multihash::Multihash;
use shared_types::PeerId;

use crate::mapping::{PeerIdMapping, PeerIdMappingError};
use crate::test_peers::{ALICE_PUBLIC_KEY, alice, bob, carol};

#[test]
fn maps_a_domain_peer_id_to_libp2p_and_back() {
    let original = alice();

    let mapped = PeerIdMapping::to_libp2p(original).expect("alice maps out");
    let returned = PeerIdMapping::from_libp2p(&mapped).expect("alice maps back");

    assert_eq!(returned, original);
}

#[test]
fn the_round_trip_holds_for_every_fixture_identity() {
    for peer in [alice(), bob(), carol()] {
        let mapped = PeerIdMapping::to_libp2p(peer).expect("maps out");
        assert_eq!(
            PeerIdMapping::from_libp2p(&mapped).expect("maps back"),
            peer
        );
    }
}

#[test]
fn maps_a_libp2p_peer_id_to_the_domain_and_back() {
    let keypair = Keypair::generate_ed25519();
    let original = keypair.public().to_peer_id();

    let domain = PeerIdMapping::from_libp2p(&original).expect("maps in");
    let returned = PeerIdMapping::to_libp2p(domain).expect("maps out");

    assert_eq!(returned, original);
}

#[test]
fn the_mapping_preserves_the_exact_key_bytes() {
    // The identity is the key (canvas §2.1). A mapping that changed a byte
    // would silently change who a message is from — invariant 4 rests on this.
    let mapped = PeerIdMapping::to_libp2p(alice()).expect("maps out");
    let returned = PeerIdMapping::from_libp2p(&mapped).expect("maps back");

    assert_eq!(returned.as_bytes(), &ALICE_PUBLIC_KEY);
}

#[test]
fn distinct_identities_stay_distinct_across_the_mapping() {
    let mapped_alice = PeerIdMapping::to_libp2p(alice()).expect("maps out");
    let mapped_bob = PeerIdMapping::to_libp2p(bob()).expect("maps out");

    assert_ne!(mapped_alice, mapped_bob);
}

#[test]
fn a_hashed_identity_is_refused_rather_than_guessed() {
    // Multihash 0x12 is SHA2-256: a peer id that is a *digest* of a key, which
    // is what libp2p produces for keys too long to inline. The key is not
    // recoverable, and inventing one would forge an identity.
    let digest = Multihash::<64>::wrap(0x12, &[0_u8; 32]).expect("wraps");
    let hashed = libp2p::PeerId::from_multihash(digest).expect("valid peer id");

    assert_eq!(
        PeerIdMapping::from_libp2p(&hashed),
        Err(PeerIdMappingError::NotInlined { code: 0x12 })
    );
}

#[test]
fn a_non_ed25519_inlined_key_is_refused() {
    // secp256k1 keys inline too, so the hash code alone does not prove the
    // identity is one this build can carry.
    let keypair = Keypair::generate_secp256k1();
    let peer = keypair.public().to_peer_id();

    assert_eq!(
        PeerIdMapping::from_libp2p(&peer),
        Err(PeerIdMappingError::NotEd25519)
    );
}

#[test]
fn an_inlined_payload_that_is_not_a_public_key_is_refused() {
    let digest = Multihash::<64>::wrap(0x00, b"not a protobuf public key").expect("wraps");
    let bogus = libp2p::PeerId::from_multihash(digest).expect("valid peer id");

    assert_eq!(
        PeerIdMapping::from_libp2p(&bogus),
        Err(PeerIdMappingError::MalformedKey)
    );
}

#[test]
fn no_input_makes_the_mapping_panic() {
    // Every shape a remote peer can choose is ordinary inbound data here.
    let inputs = [
        libp2p::PeerId::random(),
        Keypair::generate_ed25519().public().to_peer_id(),
        Keypair::generate_secp256k1().public().to_peer_id(),
        libp2p::PeerId::from_multihash(Multihash::<64>::wrap(0x00, &[]).expect("wraps"))
            .expect("valid"),
        libp2p::PeerId::from_multihash(Multihash::<64>::wrap(0x12, &[9_u8; 32]).expect("wraps"))
            .expect("valid"),
    ];

    for input in inputs {
        let _ = PeerIdMapping::from_libp2p(&input);
    }
}

#[test]
fn a_libp2p_public_key_and_the_domain_peer_id_agree() {
    // The two crates must derive the same identity from the same key bytes,
    // or a signature verified here would be attributed to a different peer.
    let key = ed25519::PublicKey::try_from_bytes(&ALICE_PUBLIC_KEY).expect("valid key");
    let from_libp2p = PublicKey::from(key).to_peer_id();
    let from_domain =
        PeerIdMapping::to_libp2p(PeerId::from_public_key_bytes(ALICE_PUBLIC_KEY).expect("valid"))
            .expect("maps out");

    assert_eq!(from_libp2p, from_domain);
}
