use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::application::commands::{InitializeLocalIdentity, InitializeLocalIdentityHandler};
use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::port_fakes::{FakeKeyStore, UnusableKeyStore};
use crate::ports::{IdentityKeyStoreError, IdentityKeyStorePort, LocalIdentityAssumption};
use crate::test_peers;

fn handler_over(
    state: &Arc<LocalIdentityState>,
    key_store: &Arc<FakeKeyStore>,
) -> InitializeLocalIdentityHandler {
    InitializeLocalIdentityHandler::new(
        Arc::clone(state),
        Arc::clone(key_store) as Arc<dyn IdentityKeyStorePort + Send + Sync>,
    )
}

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

#[test]
fn a_fresh_install_creates_the_keypair_and_announces_the_identity() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));

    let assumption = handler_over(&state, &key_store)
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("first launch needs no configuration (AC1)");

    let event = assumption
        .event()
        .expect("the first launch announces itself");
    assert_eq!(event.peer, test_peers::alice());
    assert_eq!(key_store.creations(), 1, "the keypair was generated");
    assert_eq!(
        state.read(LocalIdentity::peer_id),
        Some(test_peers::alice())
    );
}

#[test]
fn a_first_launch_derives_its_display_name_rather_than_asking_the_user() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));

    let assumption = handler_over(&state, &key_store)
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("no interaction is required");

    assert_eq!(
        assumption.event().expect("assumed").display_name,
        DisplayName::derived_from(&test_peers::alice()),
        "zero-interaction first launch (AC1) means the name is derived, never prompted"
    );
}

#[test]
fn a_supplied_display_name_is_used_instead_of_the_derived_one() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));

    let assumption = handler_over(&state, &key_store)
        .handle(InitializeLocalIdentity::named(name("Ada")))
        .expect("initialize");

    assert_eq!(
        assumption.event().expect("assumed").display_name,
        name("Ada")
    );
}

#[test]
fn initializing_twice_in_one_process_emits_no_second_event_and_touches_no_store() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));
    let handler = handler_over(&state, &key_store);

    let first = handler
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("first call");
    let second = handler
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("second call is harmless");

    assert!(matches!(first, LocalIdentityAssumption::Assumed(_)));
    assert_eq!(
        second,
        LocalIdentityAssumption::AlreadyAssumed(test_peers::alice()),
        "the second call reports the identity, not a second creation"
    );
    assert_eq!(first.peer(), second.peer());
    assert_eq!(
        key_store.loads(),
        1,
        "an idempotent bootstrap re-reads nothing"
    );
}

#[test]
fn re_initializing_never_renames_the_peer() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));
    let handler = handler_over(&state, &key_store);

    handler
        .handle(InitializeLocalIdentity::named(name("Ada")))
        .expect("first call");
    handler
        .handle(InitializeLocalIdentity::named(name("Grace")))
        .expect("second call");

    assert_eq!(
        state.read(|identity| identity.display_name().clone()),
        Some(name("Ada")),
        "renaming is SetDisplayName's job, not a side effect of bootstrapping"
    );
}

#[test]
fn a_restart_reassumes_the_same_peer_without_creating_a_second_keypair() {
    let key_store = Arc::new(FakeKeyStore::empty(test_peers::alice()));

    let first_run = Arc::new(LocalIdentityState::uninitialized());
    let before = handler_over(&first_run, &key_store)
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("first launch");

    let second_run = Arc::new(LocalIdentityState::uninitialized());
    let after = handler_over(&second_run, &key_store)
        .handle(InitializeLocalIdentity::with_derived_display_name())
        .expect("later launch");

    assert_eq!(
        before.peer(),
        after.peer(),
        "PeerId is stable across restarts (AC9)"
    );
    assert_eq!(
        key_store.creations(),
        1,
        "a restart loads, it does not regenerate"
    );
    assert_eq!(
        key_store.loads(),
        2,
        "each process asked the store exactly once"
    );
    assert!(
        after.event().is_some(),
        "a new process does assume the identity, and says so"
    );
}

#[test]
fn a_key_store_failure_surfaces_as_a_typed_error_and_leaves_no_identity() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let key_store: Arc<dyn IdentityKeyStorePort + Send + Sync> =
        Arc::new(UnusableKeyStore(IdentityKeyStoreError::Corrupt));
    let handler = InitializeLocalIdentityHandler::new(Arc::clone(&state), key_store);

    let outcome = handler.handle(InitializeLocalIdentity::with_derived_display_name());

    assert_eq!(outcome, Err(IdentityKeyStoreError::Corrupt));
    assert_eq!(
        state.read(LocalIdentity::peer_id),
        None,
        "a failed bootstrap must not leave a half-assumed identity behind"
    );
}

#[test]
fn every_key_store_failure_is_reported_rather_than_panicking() {
    let failures = [
        IdentityKeyStoreError::Unreadable,
        IdentityKeyStoreError::Corrupt,
        IdentityKeyStoreError::UnsupportedSchemaVersion { found: 9 },
        IdentityKeyStoreError::CreationFailed,
    ];

    for failure in failures {
        let state = Arc::new(LocalIdentityState::uninitialized());
        let key_store: Arc<dyn IdentityKeyStorePort + Send + Sync> =
            Arc::new(UnusableKeyStore(failure));
        let handler = InitializeLocalIdentityHandler::new(state, key_store);

        assert_eq!(
            handler.handle(InitializeLocalIdentity::with_derived_display_name()),
            Err(failure)
        );
    }
}
