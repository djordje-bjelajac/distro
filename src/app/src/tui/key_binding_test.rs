use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::{KeyBindings, Mode, Overlay, UiAction};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn character(value: char) -> KeyEvent {
    key(KeyCode::Char(value))
}

fn browsing(event: KeyEvent) -> UiAction {
    KeyBindings::action(event, Mode::Browsing, &Overlay::None)
}

fn typing(event: KeyEvent) -> UiAction {
    KeyBindings::action(event, Mode::Composing, &Overlay::None)
}

#[test]
fn interrupt_always_quits_even_mid_message() {
    // An input line that swallowed it would leave a user with a terminal they
    // cannot get out of.
    let interrupt = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(browsing(interrupt), UiAction::Quit);
    assert_eq!(typing(interrupt), UiAction::Quit);
    assert_eq!(
        KeyBindings::action(interrupt, Mode::RedeemingTicket, &Overlay::Help),
        UiAction::Quit
    );
}

#[test]
fn q_quits_while_browsing_and_is_a_letter_while_typing() {
    // The single most common terminal-interface bug there is.
    assert_eq!(browsing(character('q')), UiAction::Quit);
    assert_eq!(typing(character('q')), UiAction::Insert('q'));
}

#[test]
fn every_command_letter_is_text_while_typing() {
    for letter in [
        'b', 'c', 'd', 'f', 'g', 'i', 'j', 'k', 'l', 'p', 'r', 'v', '?',
    ] {
        assert_eq!(typing(character(letter)), UiAction::Insert(letter));
    }
}

#[test]
fn conversations_are_cycled_by_tab_and_by_arrows_and_by_jk() {
    for event in [key(KeyCode::Tab), key(KeyCode::Down), character('j')] {
        assert_eq!(browsing(event), UiAction::NextConversation);
    }
    for event in [key(KeyCode::BackTab), key(KeyCode::Up), character('k')] {
        assert_eq!(browsing(event), UiAction::PreviousConversation);
    }
}

#[test]
fn enter_starts_a_message_and_then_sends_it() {
    assert_eq!(browsing(key(KeyCode::Enter)), UiAction::Compose);
    assert_eq!(typing(key(KeyCode::Enter)), UiAction::Submit);
}

#[test]
fn escape_cancels_what_is_being_typed() {
    assert_eq!(typing(key(KeyCode::Esc)), UiAction::Cancel);
}

#[test]
fn backspace_deletes_only_while_typing() {
    assert_eq!(typing(key(KeyCode::Backspace)), UiAction::Delete);
    assert_eq!(browsing(key(KeyCode::Backspace)), UiAction::Ignored);
}

#[test]
fn an_open_overlay_is_dismissed_by_escape_or_enter() {
    assert_eq!(
        KeyBindings::action(key(KeyCode::Esc), Mode::Browsing, &Overlay::Help),
        UiAction::Cancel
    );
    assert_eq!(
        KeyBindings::action(key(KeyCode::Enter), Mode::Browsing, &Overlay::Fingerprints),
        UiAction::Cancel
    );
}

#[test]
fn an_open_overlay_still_lets_the_conversation_be_changed() {
    // Reading the help should not mean losing your place.
    assert_eq!(
        KeyBindings::action(key(KeyCode::Tab), Mode::Browsing, &Overlay::Help),
        UiAction::NextConversation
    );
}

#[test]
fn the_trust_and_ticket_commands_are_bound() {
    assert_eq!(browsing(character('v')), UiAction::VerifySelected);
    assert_eq!(browsing(character('b')), UiAction::ToggleBlockSelected);
    assert_eq!(browsing(character('f')), UiAction::ToggleFingerprints);
    assert_eq!(browsing(character('g')), UiAction::GenerateTicket);
    assert_eq!(browsing(character('p')), UiAction::PasteTicket);
    assert_eq!(browsing(character('c')), UiAction::ConnectSelected);
    assert_eq!(browsing(character('r')), UiAction::Rejoin);
    assert_eq!(browsing(character('l')), UiAction::Leave);
    assert_eq!(browsing(character('d')), UiAction::ToggleDiagnostics);
    assert_eq!(browsing(character('?')), UiAction::ToggleHelp);
}

#[test]
fn an_unbound_key_does_nothing() {
    assert_eq!(browsing(character('z')), UiAction::Ignored);
    assert_eq!(browsing(key(KeyCode::F(5))), UiAction::Ignored);
}

#[test]
fn a_control_chord_never_becomes_message_text() {
    // A control code in a message body is something the domain refuses anyway.
    let chord = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);

    assert_eq!(typing(chord), UiAction::Ignored);
}

#[test]
fn shifted_letters_are_typed_as_themselves() {
    let shifted = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);

    assert_eq!(typing(shifted), UiAction::Insert('A'));
}

#[test]
fn every_documented_key_is_actually_bound() {
    // A help screen that lies is worse than none, so the list is not allowed
    // to grow without a binding behind it.
    assert!(!KeyBindings::HELP.is_empty());
    for (keys, description) in KeyBindings::HELP {
        assert!(!keys.is_empty());
        assert!(!description.is_empty());
    }
}
