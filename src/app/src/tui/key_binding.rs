use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::{Mode, Overlay};

/// What one keystroke means.
///
/// # Why the mapping is a pure function
///
/// Every terminal interface bug that is not a drawing bug is a keystroke that
/// went to the wrong place: a `q` that quit while the user was typing a
/// message, an `Escape` that closed an overlay *and* cancelled the message
/// behind it, a `Ctrl+C` that was swallowed by an input line. None of that
/// needs a terminal to reproduce — a `KeyEvent` is a plain value — so the whole
/// mapping lives here and is asserted rather than tried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    /// This key means nothing here.
    Ignored,
    Quit,
    NextConversation,
    PreviousConversation,
    /// Start typing a message into the selected conversation.
    Compose,
    /// Abandon whatever is being typed, or close an overlay.
    Cancel,
    /// Send what was typed, or redeem the pasted ticket.
    Submit,
    Insert(char),
    Delete,
    ToggleHelp,
    /// Show the full fingerprints for the out-of-band comparison (AC6).
    ToggleFingerprints,
    ToggleDiagnostics,
    /// Mint a join ticket for someone else to redeem (D1).
    GenerateTicket,
    /// Start pasting a join ticket.
    PasteTicket,
    /// Record that the selected peer's fingerprint was compared and matched.
    VerifySelected,
    /// Block or unblock the selected peer (invariant 11).
    ToggleBlockSelected,
    /// Dial the selected peer.
    ConnectSelected,
    /// Walk the bootstrap ladder again.
    Rejoin,
    /// Close every session and announce the departure.
    Leave,
}

/// The key map.
pub struct KeyBindings;

impl KeyBindings {
    /// The keys, as the help overlay lists them.
    ///
    /// Held here rather than written out in the overlay so a binding cannot be
    /// changed without the help text changing with it — a help screen that
    /// lies is worse than none.
    pub const HELP: [(&'static str, &'static str); 13] = [
        ("Tab / ↓ / j", "next conversation"),
        ("Shift+Tab / ↑ / k", "previous conversation"),
        ("i / Enter", "write a message"),
        ("Enter", "send"),
        ("Esc", "cancel, or close an overlay"),
        ("c", "connect to the selected peer"),
        ("v", "verify the selected peer (compare fingerprints first)"),
        ("b", "block or unblock the selected peer"),
        ("f", "show full fingerprints"),
        ("g", "generate a join ticket to hand out"),
        ("p", "paste a join ticket and join with it"),
        ("r / l", "rejoin / leave the network"),
        ("d / ? / q", "diagnostics / help / quit"),
    ];

    /// Maps one keystroke, given what the interface is currently doing.
    pub fn action(key: KeyEvent, mode: Mode, overlay: &Overlay) -> UiAction {
        // Interrupt always means interrupt. An input line that swallowed it
        // would leave a user with a terminal they cannot get out of.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            return UiAction::Quit;
        }

        match mode {
            Mode::Composing | Mode::RedeemingTicket => Self::typing(key),
            Mode::Browsing => Self::browsing(key, overlay),
        }
    }

    /// While typing, almost everything is text.
    fn typing(key: KeyEvent) -> UiAction {
        match key.code {
            KeyCode::Esc => UiAction::Cancel,
            KeyCode::Enter => UiAction::Submit,
            KeyCode::Backspace => UiAction::Delete,
            // Anything with Alt or Control is a chord, not a character; a
            // terminal that reported one as text would put a control code in a
            // message body, which the domain refuses anyway.
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                UiAction::Insert(character)
            }
            _ => UiAction::Ignored,
        }
    }

    /// While browsing, keys are commands.
    fn browsing(key: KeyEvent, overlay: &Overlay) -> UiAction {
        // An open overlay takes the two keys that dismiss it and passes
        // everything else through, so a user reading the help can still change
        // conversation.
        if !matches!(overlay, Overlay::None) && matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            return UiAction::Cancel;
        }

        match key.code {
            KeyCode::Tab | KeyCode::Down => UiAction::NextConversation,
            KeyCode::BackTab | KeyCode::Up => UiAction::PreviousConversation,
            KeyCode::Enter => UiAction::Compose,
            KeyCode::Esc => UiAction::Cancel,
            KeyCode::Char(character) => Self::command(character),
            _ => UiAction::Ignored,
        }
    }

    fn command(character: char) -> UiAction {
        match character {
            'j' => UiAction::NextConversation,
            'k' => UiAction::PreviousConversation,
            'i' => UiAction::Compose,
            'q' => UiAction::Quit,
            '?' => UiAction::ToggleHelp,
            'f' => UiAction::ToggleFingerprints,
            'd' => UiAction::ToggleDiagnostics,
            'g' => UiAction::GenerateTicket,
            'p' => UiAction::PasteTicket,
            'v' => UiAction::VerifySelected,
            'b' => UiAction::ToggleBlockSelected,
            'c' => UiAction::ConnectSelected,
            'r' => UiAction::Rejoin,
            'l' => UiAction::Leave,
            _ => UiAction::Ignored,
        }
    }
}
