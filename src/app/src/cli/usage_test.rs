use crate::cli::Usage;

#[test]
fn the_help_text_states_the_connectivity_limit_s7_requires() {
    let text = Usage::text().to_lowercase();

    assert!(text.contains("symmetric nat"), "S7 limit is not stated");
    assert!(
        text.contains("relay"),
        "S7's relay condition is not explained"
    );
}

#[test]
fn the_help_text_states_the_privacy_disclosure_s8_requires() {
    let text = Usage::text().to_lowercase();

    assert!(
        text.contains("announces this peer's network addresses"),
        "S8's announcement disclosure is not stated"
    );
    assert!(
        text.contains("readable by every member"),
        "S8's broadcast disclosure is not stated"
    );
}

#[test]
fn the_help_text_names_every_way_of_choosing_a_profile_directory() {
    let text = Usage::text();

    assert!(text.contains("--profile"));
    assert!(text.contains("DISTRO_PROFILE_DIR"));
    assert!(text.contains("$HOME/.local/share/distro"));
}

#[test]
fn the_help_text_names_no_bootstrap_host() {
    // S1: no operator-run host may enter any code path, and none may be
    // suggested to a user either.
    let text = Usage::text();

    assert!(!text.contains("bootstrap.") && !text.contains("://"));
}

#[test]
fn the_listen_option_says_plainly_that_it_is_not_a_bootstrap_list() {
    // The one option whose shape could be mistaken for the thing S1 forbids.
    let text = Usage::text();

    assert!(text.contains("--listen"));
    assert!(
        text.contains("Not a bootstrap"),
        "the distinction between binding and contacting must be stated"
    );
}

#[test]
fn the_ui_disclosures_repeat_both_safeguards() {
    let overlay = Usage::DISCLOSURES.join(" ").to_lowercase();

    assert!(overlay.contains("announces your addresses"));
    assert!(overlay.contains("broadcast messages are readable"));
    assert!(overlay.contains("symmetric-nat"));
}
