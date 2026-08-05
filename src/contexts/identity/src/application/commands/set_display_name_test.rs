use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::application::commands::{SetDisplayName, SetDisplayNameHandler};
use crate::domain::events::DisplayNameChanged;
use crate::domain::{DisplayName, DisplayNameError, LocalIdentity};
use crate::ports::SetDisplayNameError;
use crate::test_peers;

fn name(raw: &str) -> DisplayName {
    DisplayName::new(raw).expect("test fixture must be a valid display name")
}

fn assumed_as(display_name: &str) -> (Arc<LocalIdentityState>, SetDisplayNameHandler) {
    let state = Arc::new(LocalIdentityState::uninitialized());
    state
        .assume_once(|| {
            Ok::<_, ()>(LocalIdentity::initialize(
                test_peers::alice(),
                name(display_name),
            ))
        })
        .expect("seeding the identity cannot fail");
    let handler = SetDisplayNameHandler::new(Arc::clone(&state));
    (state, handler)
}

fn current_name(state: &LocalIdentityState) -> Option<DisplayName> {
    state.read(|identity| identity.display_name().clone())
}

#[test]
fn renaming_reports_both_sides_of_the_change() {
    let (state, handler) = assumed_as("Ada");

    let event = handler
        .handle(SetDisplayName::new("Grace"))
        .expect("a valid name is accepted");

    assert_eq!(
        event,
        Some(DisplayNameChanged {
            peer: test_peers::alice(),
            previous: name("Ada"),
            current: name("Grace"),
        })
    );
    assert_eq!(current_name(&state), Some(name("Grace")));
}

#[test]
fn setting_the_name_it_already_has_emits_nothing() {
    let (state, handler) = assumed_as("Ada");

    let event = handler
        .handle(SetDisplayName::new("Ada"))
        .expect("accepted");

    assert_eq!(event, None, "no change occurred, so no change is announced");
    assert_eq!(current_name(&state), Some(name("Ada")));
}

#[test]
fn a_padding_only_difference_is_still_a_no_op() {
    let (_, handler) = assumed_as("Ada");

    let event = handler
        .handle(SetDisplayName::new("  Ada\n"))
        .expect("padding is trimmed, not rejected");

    assert_eq!(
        event, None,
        "the value object trims, so this is the same name"
    );
}

#[test]
fn an_invalid_name_is_rejected_through_the_value_object_and_changes_nothing() {
    let (state, handler) = assumed_as("Ada");

    let too_long = "x".repeat(DisplayName::MAX_SCALAR_VALUES + 1);
    let rejections = [
        ("   ", DisplayNameError::Empty),
        ("bad\u{7}name", DisplayNameError::ContainsControlCharacter),
        (
            too_long.as_str(),
            DisplayNameError::TooLong {
                scalar_values: DisplayName::MAX_SCALAR_VALUES + 1,
                limit: DisplayName::MAX_SCALAR_VALUES,
            },
        ),
    ];

    for (requested, expected) in rejections {
        assert_eq!(
            handler.handle(SetDisplayName::new(requested)),
            Err(SetDisplayNameError::Invalid(expected))
        );
    }
    assert_eq!(current_name(&state), Some(name("Ada")));
}

#[test]
fn renaming_before_the_identity_exists_is_a_typed_error_not_a_panic() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let handler = SetDisplayNameHandler::new(Arc::clone(&state));

    assert_eq!(
        handler.handle(SetDisplayName::new("Ada")),
        Err(SetDisplayNameError::NotInitialized)
    );
    assert_eq!(current_name(&state), None);
}

#[test]
fn an_invalid_name_is_rejected_before_the_identity_is_even_consulted() {
    let state = Arc::new(LocalIdentityState::uninitialized());
    let handler = SetDisplayNameHandler::new(state);

    assert_eq!(
        handler.handle(SetDisplayName::new("")),
        Err(SetDisplayNameError::Invalid(DisplayNameError::Empty)),
        "validation is the value object's answer regardless of what state holds"
    );
}
