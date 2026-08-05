use std::sync::Arc;

use shared_types::Fingerprint;

use crate::application::LocalIdentityState;
use crate::application::queries::{GetLocalIdentity, GetLocalIdentityHandler};
use crate::domain::{DisplayName, LocalIdentity};
use crate::ports::LocalIdentitySummary;
use crate::test_peers;

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

fn assumed() -> (Arc<LocalIdentityState>, GetLocalIdentityHandler) {
    let state = Arc::new(LocalIdentityState::uninitialized());
    state
        .assume_once(|| Ok::<_, ()>(LocalIdentity::initialize(test_peers::alice(), name("Ada"))))
        .expect("seeding the identity cannot fail");
    let handler = GetLocalIdentityHandler::new(Arc::clone(&state));
    (state, handler)
}

#[test]
fn reports_the_peer_its_name_and_the_fingerprint_for_out_of_band_verification() {
    let (_, handler) = assumed();

    let summary = handler
        .handle(GetLocalIdentity)
        .expect("identity is assumed");

    assert_eq!(
        summary,
        LocalIdentitySummary {
            peer: test_peers::alice(),
            display_name: name("Ada"),
            fingerprint: Fingerprint::of(&test_peers::alice()),
        },
        "AC6 needs the fingerprint to be readable without deriving it at the call site"
    );
}

#[test]
fn reports_nothing_before_the_identity_is_assumed() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let handler = GetLocalIdentityHandler::new(state);

    assert_eq!(handler.handle(GetLocalIdentity), None);
}

#[test]
fn reading_repeatedly_changes_nothing() {
    let (state, handler) = assumed();

    let first = handler.handle(GetLocalIdentity);
    let second = handler.handle(GetLocalIdentity);
    let third = handler.handle(GetLocalIdentity);

    assert_eq!(first, second);
    assert_eq!(second, third);
    assert_eq!(
        state.read(|identity| identity.display_name().clone()),
        Some(name("Ada")),
        "a query path never mutates the state it reads"
    );
}
