use messaging::domain::ConversationId;

use crate::test_peers::{alice, bob, carol};
use crate::tui::{ConversationEntry, Mode, Overlay, PeerLabels, UiState};

#[test]
fn a_fresh_interface_is_browsing_the_first_conversation() {
    let state = UiState::new();

    assert_eq!(state.mode(), Mode::Browsing);
    assert_eq!(state.selected(), 0);
    assert!(!state.is_typing());
    assert!(!state.is_quitting());
}

#[test]
fn the_selection_wraps_in_both_directions() {
    let mut state = UiState::new();

    state.select_next(3);
    state.select_next(3);
    assert_eq!(state.selected(), 2);

    state.select_next(3);
    assert_eq!(state.selected(), 0);

    state.select_previous(3);
    assert_eq!(state.selected(), 2);
}

#[test]
fn a_selection_cannot_survive_the_row_it_pointed_at() {
    // Peers appear and disappear while a user is looking at the screen; a
    // stale selection would send the next message to whoever took its place.
    let mut state = UiState::new();
    state.select_next(5);
    state.select_next(5);
    state.select_next(5);
    assert_eq!(state.selected(), 3);

    state.clamp_selection(2);

    assert_eq!(state.selected(), 1);
}

#[test]
fn an_empty_list_selects_nothing_rather_than_panicking() {
    let mut state = UiState::new();

    state.select_next(0);
    state.select_previous(0);
    state.clamp_selection(0);

    assert_eq!(state.selected(), 0);
}

#[test]
fn typed_characters_reach_the_input_line() {
    let mut state = UiState::new();
    state.compose();

    for character in "hello".chars() {
        state.insert(character);
    }

    assert!(state.is_typing());
    assert_eq!(state.input(), "hello");
}

#[test]
fn backspace_removes_the_last_character() {
    let mut state = UiState::new();
    state.compose();
    state.insert('a');
    state.insert('b');

    state.delete();

    assert_eq!(state.input(), "a");
}

#[test]
fn backspace_on_an_empty_line_does_nothing() {
    let mut state = UiState::new();
    state.compose();

    state.delete();

    assert_eq!(state.input(), "");
}

#[test]
fn submitting_returns_the_trimmed_text_and_goes_back_to_browsing() {
    let mut state = UiState::new();
    state.compose();
    for character in "  hello  ".chars() {
        state.insert(character);
    }

    let submitted = state.submit();

    assert_eq!(submitted, Some("hello".to_owned()));
    assert_eq!(state.mode(), Mode::Browsing);
    assert_eq!(state.input(), "");
}

#[test]
fn submitting_nothing_sends_nothing() {
    // An empty body is not a message and the domain would refuse it anyway;
    // refusing it here keeps a stray Enter from becoming a notice.
    let mut state = UiState::new();
    state.compose();
    state.insert(' ');

    assert_eq!(state.submit(), None);
}

#[test]
fn cancelling_discards_what_was_typed() {
    // A half-typed message that reappeared on the next compose would
    // eventually be sent by accident — possibly to a different peer.
    let mut state = UiState::new();
    state.compose();
    state.insert('x');

    state.cancel();

    assert_eq!(state.mode(), Mode::Browsing);
    assert_eq!(state.input(), "");
}

#[test]
fn composing_starts_from_an_empty_line_every_time() {
    let mut state = UiState::new();
    state.compose();
    state.insert('x');
    state.submit();

    state.compose();

    assert_eq!(state.input(), "");
}

#[test]
fn a_ticket_is_pasted_in_its_own_mode() {
    // A pasted ticket must not be able to become a broadcast message.
    let mut state = UiState::new();

    state.redeem_ticket();

    assert_eq!(state.mode(), Mode::RedeemingTicket);
    assert!(state.is_typing());
}

#[test]
fn the_input_line_is_bounded() {
    let mut state = UiState::new();
    state.compose();

    for _ in 0..(UiState::MAX_INPUT_BYTES + 100) {
        state.insert('x');
    }

    assert_eq!(state.input().len(), UiState::MAX_INPUT_BYTES);
}

#[test]
fn an_overlay_opens_and_closes() {
    let mut state = UiState::new();

    state.show(Overlay::Help);
    assert_eq!(state.overlay(), &Overlay::Help);

    assert!(state.close_overlay());
    assert_eq!(state.overlay(), &Overlay::None);
    assert!(!state.close_overlay());
}

#[test]
fn the_same_overlay_key_twice_closes_it() {
    let mut state = UiState::new();

    state.toggle(Overlay::Fingerprints);
    state.toggle(Overlay::Fingerprints);

    assert_eq!(state.overlay(), &Overlay::None);
}

#[test]
fn a_ticket_overlay_toggles_by_kind_not_by_content() {
    // Pressing `g` again while a ticket is on screen closes it rather than
    // minting a second one on top of the first.
    let mut state = UiState::new();

    state.toggle(Overlay::Ticket("distro-join-1.a".to_owned()));
    state.toggle(Overlay::Ticket("distro-join-1.b".to_owned()));

    assert_eq!(state.overlay(), &Overlay::None);
}

#[test]
fn starting_to_type_closes_any_overlay() {
    let mut state = UiState::new();
    state.show(Overlay::Help);

    state.compose();

    assert_eq!(state.overlay(), &Overlay::None);
}

#[test]
fn the_conversation_list_starts_with_the_broadcast_channel() {
    let entries = ConversationEntry::list(&[bob(), carol()], PeerLabels::for_local(alice()));

    assert_eq!(entries[0].id, ConversationId::Broadcast);
    assert_eq!(entries[0].label, "broadcast");
    assert_eq!(entries[0].counterpart(), None);
}

#[test]
fn every_known_peer_can_be_talked_to_before_anything_is_said() {
    // `MessagingQueryPort::conversations` lists only those with recorded
    // history, which is the right answer to a different question.
    let entries = ConversationEntry::list(&[bob(), carol()], PeerLabels::for_local(alice()));

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[1].counterpart(), Some(bob()));
    assert_eq!(entries[2].counterpart(), Some(carol()));
}

#[test]
fn a_direct_conversation_is_labelled_by_its_peers_fingerprint() {
    let entries = ConversationEntry::list(&[bob()], PeerLabels::for_local(alice()));

    assert!(entries[1].label.contains(&PeerLabels::short(bob())));
}

#[test]
fn with_no_known_peers_only_the_broadcast_channel_exists() {
    let entries = ConversationEntry::list(&[], PeerLabels::for_local(alice()));

    assert_eq!(entries.len(), 1);
}
