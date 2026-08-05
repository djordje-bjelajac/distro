use crate::application::LocalIdentityState;
use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::{IdentityKeyStoreError, LocalIdentityAssumption};
use crate::test_peers;

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

fn alice() -> Result<
    (
        LocalIdentity,
        crate::domain::events::LocalIdentityInitialized,
    ),
    (),
> {
    Ok(LocalIdentity::initialize(test_peers::alice(), name("Ada")))
}

#[test]
fn an_uninitialized_state_holds_nothing_and_refuses_to_be_read() {
    let state = LocalIdentityState::uninitialized();

    assert_eq!(state.read(LocalIdentity::peer_id), None);
    assert_eq!(state.read(|identity| identity.peer_id()), None);
    assert_eq!(
        state.modify(|_| unreachable!("nothing to modify")),
        None::<()>
    );
}

#[test]
fn only_the_first_assumption_installs_an_identity() {
    let state = LocalIdentityState::uninitialized();

    let first = state.assume_once(alice).expect("first assumption");
    let second = state
        .assume_once(|| -> Result<_, ()> { unreachable!("the state is already assumed") })
        .expect("second assumption");

    assert!(matches!(first, LocalIdentityAssumption::Assumed(_)));
    assert_eq!(
        second,
        LocalIdentityAssumption::AlreadyAssumed(test_peers::alice()),
        "the closure is not even called once an identity is installed"
    );
}

#[test]
fn a_failed_assumption_installs_nothing() {
    let state = LocalIdentityState::uninitialized();

    let outcome = state.assume_once(|| Err(IdentityKeyStoreError::Unreadable));

    assert_eq!(outcome, Err(IdentityKeyStoreError::Unreadable));
    assert_eq!(state.read(LocalIdentity::peer_id), None);
    assert!(
        state.assume_once(alice).is_ok(),
        "a later attempt can still succeed: nothing was half-installed"
    );
}

#[test]
fn modifying_an_installed_identity_is_visible_to_the_next_read() {
    let state = LocalIdentityState::uninitialized();
    state.assume_once(alice).expect("assumption");

    let changed = state
        .modify(|identity| identity.change_display_name(name("Grace")))
        .expect("an installed identity can be modified");

    assert!(changed.is_some());
    assert_eq!(
        state.read(|identity| identity.display_name().clone()),
        Some(name("Grace"))
    );
}
