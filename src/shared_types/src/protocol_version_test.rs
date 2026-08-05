use crate::ProtocolVersion;

#[test]
fn current_version_is_1_0() {
    assert_eq!(ProtocolVersion::CURRENT.major, 1);
    assert_eq!(ProtocolVersion::CURRENT.minor, 0);
}

#[test]
fn new_sets_major_and_minor() {
    let version = ProtocolVersion::new(3, 7);
    assert_eq!(version.major, 3);
    assert_eq!(version.minor, 7);
}

#[test]
fn equality_is_by_major_and_minor() {
    assert_eq!(ProtocolVersion::new(1, 2), ProtocolVersion::new(1, 2));
    assert_ne!(ProtocolVersion::new(1, 2), ProtocolVersion::new(1, 3));
    assert_ne!(ProtocolVersion::new(1, 2), ProtocolVersion::new(2, 2));
}
