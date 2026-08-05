use std::error::Error as _;

use crate::domain::DisplayName;
use crate::ports::SetDisplayNameError;

#[test]
fn wraps_a_display_name_rejection_and_keeps_it_as_the_source() {
    let rejection = DisplayName::new("bad\u{7}name").expect_err("control characters are rejected");

    let error = SetDisplayNameError::from(rejection);

    assert_eq!(error, SetDisplayNameError::Invalid(rejection));
    assert_eq!(error.to_string(), rejection.to_string());
    assert!(
        error.source().is_some(),
        "the domain rejection is preserved"
    );
}

#[test]
fn naming_a_peer_that_does_not_exist_yet_reads_as_its_own_case() {
    let error = SetDisplayNameError::NotInitialized;

    assert_eq!(
        error.to_string(),
        "local identity has not been initialized yet"
    );
    assert!(error.source().is_none());
    let _: &dyn std::error::Error = &error;
}
