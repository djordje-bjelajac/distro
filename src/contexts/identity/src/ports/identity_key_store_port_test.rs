use crate::ports::port_fakes::{FakeKeyStore, UnusableKeyStore};
use crate::ports::{IdentityKeyStoreError, IdentityKeyStorePort};
use crate::test_peers;

#[test]
fn load_or_create_returns_the_same_peer_on_every_call() {
    let store = FakeKeyStore::holding(test_peers::alice());
    let port: &dyn IdentityKeyStorePort = &store;

    let first = port
        .load_or_create_local_peer()
        .expect("first launch creates the keypair");
    let second = port
        .load_or_create_local_peer()
        .expect("later calls load the same keypair");

    assert_eq!(first, test_peers::alice());
    assert_eq!(first, second, "PeerId is stable across calls (AC9)");
    assert_eq!(store.loads(), 2, "both calls went through the port");
}

#[test]
fn reports_every_failure_as_a_typed_error_rather_than_a_panic() {
    let failures = [
        IdentityKeyStoreError::Unreadable,
        IdentityKeyStoreError::Corrupt,
        IdentityKeyStoreError::UnsupportedSchemaVersion { found: 2 },
        IdentityKeyStoreError::CreationFailed,
    ];

    for failure in failures {
        let store = UnusableKeyStore(failure);
        let port: &dyn IdentityKeyStorePort = &store;

        assert_eq!(port.load_or_create_local_peer(), Err(failure));
    }
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            IdentityKeyStoreError::Unreadable,
            "local key store could not be read",
        ),
        (
            IdentityKeyStoreError::Corrupt,
            "local key store does not contain a usable keypair",
        ),
        (
            IdentityKeyStoreError::UnsupportedSchemaVersion { found: 7 },
            "local key store has unsupported schema version 7",
        ),
        (
            IdentityKeyStoreError::CreationFailed,
            "local keypair could not be created",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
