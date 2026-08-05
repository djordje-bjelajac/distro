use crate::domain::{DisplayName, DisplayNameError};
use crate::test_peers;

fn repeat_scalar(scalar: char, count: usize) -> String {
    std::iter::repeat_n(scalar, count).collect()
}

#[test]
fn accepts_a_single_scalar_value_name() {
    let name = DisplayName::new("a").expect("one scalar value is the minimum, not a violation");
    assert_eq!(name.as_str(), "a");
}

#[test]
fn accepts_exactly_sixty_four_scalar_values() {
    let raw = repeat_scalar('a', DisplayName::MAX_SCALAR_VALUES);

    let name = DisplayName::new(&raw).expect("the upper bound is inclusive");
    assert_eq!(name.as_str().chars().count(), 64);
}

#[test]
fn rejects_sixty_five_scalar_values() {
    let raw = repeat_scalar('a', DisplayName::MAX_SCALAR_VALUES + 1);

    assert_eq!(
        DisplayName::new(&raw),
        Err(DisplayNameError::TooLong {
            scalar_values: 65,
            limit: 64,
        })
    );
}

#[test]
fn counts_scalar_values_not_bytes() {
    // Each of these is one scalar value encoded in four UTF-8 bytes: a
    // byte-length limit would reject this name at 256 bytes.
    let raw = repeat_scalar('\u{1f600}', DisplayName::MAX_SCALAR_VALUES);
    assert_eq!(raw.len(), 256);

    let name = DisplayName::new(&raw).expect("64 emoji are 64 scalar values");
    assert_eq!(name.as_str().chars().count(), 64);
}

#[test]
fn counts_every_scalar_value_of_a_zero_width_joiner_sequence() {
    // "family: man, woman, girl" is one grapheme but five scalar values
    // (three emoji joined by two U+200D zero-width joiners).
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    assert_eq!(family.chars().count(), 5);

    let twelve = family.repeat(12); // 60 scalar values
    DisplayName::new(&twelve).expect("60 scalar values are within the limit");

    let thirteen = family.repeat(13); // 65 scalar values
    assert_eq!(
        DisplayName::new(&thirteen),
        Err(DisplayNameError::TooLong {
            scalar_values: 65,
            limit: 64,
        })
    );
}

#[test]
fn zero_width_joiners_are_not_control_characters() {
    let name = DisplayName::new("\u{1f468}\u{200d}\u{1f469}")
        .expect("U+200D is a format character, not a control character");
    assert_eq!(name.as_str().chars().count(), 3);
}

#[test]
fn trims_surrounding_whitespace() {
    let name = DisplayName::new("  Ada Lovelace \t\n ").expect("surrounding whitespace is trimmed");
    assert_eq!(name.as_str(), "Ada Lovelace");
}

#[test]
fn measures_length_after_trimming() {
    let raw = format!(
        "   {}   ",
        repeat_scalar('a', DisplayName::MAX_SCALAR_VALUES)
    );
    assert!(raw.chars().count() > DisplayName::MAX_SCALAR_VALUES);

    DisplayName::new(&raw).expect("padding must not count towards the limit");
}

#[test]
fn rejects_an_empty_name() {
    assert_eq!(DisplayName::new(""), Err(DisplayNameError::Empty));
}

#[test]
fn rejects_a_whitespace_only_name() {
    assert_eq!(DisplayName::new("  \t \n "), Err(DisplayNameError::Empty));
}

#[test]
fn rejects_interior_control_characters() {
    for raw in ["line\nbreak", "bell\u{7}", "null\u{0}byte", "delete\u{7f}"] {
        assert_eq!(
            DisplayName::new(raw),
            Err(DisplayNameError::ContainsControlCharacter),
            "{raw:?} must be rejected"
        );
    }
}

#[test]
fn equality_is_by_trimmed_text() {
    let padded = DisplayName::new(" Ada ").unwrap();
    let bare = DisplayName::new("Ada").unwrap();
    let other = DisplayName::new("Grace").unwrap();

    assert_eq!(padded, bare);
    assert_ne!(padded, other);
}

#[test]
fn displays_the_trimmed_text() {
    let name = DisplayName::new(" Ada ").unwrap();
    assert_eq!(name.to_string(), "Ada");
}

#[test]
fn derives_a_valid_zero_interaction_default_from_a_peer_id() {
    let alice = DisplayName::derived_from(&test_peers::alice());
    let bob = DisplayName::derived_from(&test_peers::bob());

    // Stable, deterministic, and re-validatable through the public constructor.
    assert_eq!(alice, DisplayName::derived_from(&test_peers::alice()));
    assert_ne!(alice, bob);
    assert_eq!(
        DisplayName::new(alice.as_str()),
        Ok(DisplayName::derived_from(&test_peers::alice()))
    );
}

#[test]
fn errors_display_a_diagnostic_and_implement_error() {
    let cases = [
        (
            DisplayNameError::Empty,
            "display name is empty after trimming",
        ),
        (
            DisplayNameError::ContainsControlCharacter,
            "display name contains a control character",
        ),
        (
            DisplayNameError::TooLong {
                scalar_values: 65,
                limit: 64,
            },
            "display name is 65 Unicode scalar values, limit is 64",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        let _: &dyn std::error::Error = &error;
    }
}
