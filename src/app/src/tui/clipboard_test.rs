use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::tui::clipboard::fakes::{RecordingClipboard, UnwritableClipboard};
use crate::tui::{ClipboardPort, TerminalClipboard};

const TICKET: &str = "distro-join-1.omlpc3N1ZXJYIA";

#[test]
fn the_sequence_is_osc_52_targeting_the_clipboard_selection() {
    let sequence = TerminalClipboard::sequence_for(TICKET);

    assert!(sequence.starts_with("\x1b]52;c;"), "{sequence:?}");
    assert!(sequence.ends_with('\x07'), "{sequence:?}");
}

/// The one part that has to be exactly right: what a terminal decodes back out
/// must be the ticket, byte for byte. A ticket that arrived with a stray
/// newline or a wrapped line would be refused by the peer it was pasted into,
/// and the user would have no way to see why.
#[test]
fn what_the_terminal_decodes_is_the_text_unchanged() {
    let sequence = TerminalClipboard::sequence_for(TICKET);

    let payload = sequence
        .trim_start_matches("\x1b]52;c;")
        .trim_end_matches('\x07');
    let decoded = STANDARD.decode(payload).expect("the payload is base64");

    assert_eq!(String::from_utf8(decoded).expect("utf-8"), TICKET);
    assert!(!sequence.contains('\n'), "no line break may be introduced");
}

#[test]
fn a_fake_clipboard_records_exactly_what_it_was_offered() {
    let clipboard = RecordingClipboard::default();

    clipboard.offer(TICKET).expect("the fake accepts");

    assert_eq!(clipboard.offered(), vec![TICKET.to_owned()]);
}

#[test]
fn a_clipboard_that_cannot_be_written_reports_it() {
    assert!(UnwritableClipboard.offer(TICKET).is_err());
}
