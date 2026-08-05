use crate::PayloadKind;

const KNOWN_KINDS: [PayloadKind; 3] = [
    PayloadKind::DirectMessage,
    PayloadKind::BroadcastMessage,
    PayloadKind::Heartbeat,
];

/// Wire-code pin: these assignments are a compatibility contract. Changing
/// any of them breaks decoding against every already-deployed peer.
#[test]
fn known_codes_are_pinned() {
    assert_eq!(PayloadKind::DirectMessage.code(), 0);
    assert_eq!(PayloadKind::BroadcastMessage.code(), 1);
    assert_eq!(PayloadKind::Heartbeat.code(), 2);
}

#[test]
fn from_code_maps_assigned_codes_to_known_kinds() {
    assert_eq!(PayloadKind::from_code(0), PayloadKind::DirectMessage);
    assert_eq!(PayloadKind::from_code(1), PayloadKind::BroadcastMessage);
    assert_eq!(PayloadKind::from_code(2), PayloadKind::Heartbeat);
}

/// S2 tolerance: an unassigned code decodes to `Unknown`, never a failure.
#[test]
fn from_code_maps_unassigned_codes_to_unknown() {
    assert_eq!(PayloadKind::from_code(3), PayloadKind::Unknown(3));
    assert_eq!(PayloadKind::from_code(999), PayloadKind::Unknown(999));
    assert_eq!(
        PayloadKind::from_code(u16::MAX),
        PayloadKind::Unknown(u16::MAX)
    );
}

#[test]
fn known_kinds_round_trip_through_their_code() {
    for kind in KNOWN_KINDS {
        assert_eq!(PayloadKind::from_code(kind.code()), kind);
    }
}

/// `Unknown` preserves its discriminant, so a peer re-encoding (e.g. gossip
/// relay) an envelope of a kind it does not understand forwards the original
/// code unchanged.
#[test]
fn unknown_preserves_its_code() {
    assert_eq!(PayloadKind::Unknown(999).code(), 999);
    assert_eq!(PayloadKind::Unknown(u16::MAX).code(), u16::MAX);
}
