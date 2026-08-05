//! The terminal interface (D8) — and the view models it is made of, which are
//! tested without one.
//!
//! # Thin on purpose
//!
//! `ratatui` appears in exactly two files here: [`screen`] draws, and
//! `tui_app` reads keys and redraws. Everything that is a *decision* — what one
//! keystroke means, where an abandoned run belongs in a conversation, what a
//! trust badge says, whether a selection is still valid — lives in a module
//! with no terminal in it and a test file beside it.
//!
//! That split is not tidiness. A render path is the worst place in an
//! application to put a rule: it is the hardest to test, the easiest to
//! duplicate, and the one place where "just show something sensible" quietly
//! becomes a second answer to a question the domain already answered. The
//! `messaging` read model is the sharpest example — it groups by author and
//! provides no order across authors, and the honest thing to draw is therefore
//! not the chat window a user expects. See [`ConversationView`].

mod conversation_view;
#[cfg(test)]
mod conversation_view_test;
mod key_binding;
#[cfg(test)]
mod key_binding_test;
mod peer_label;
#[cfg(test)]
mod peer_label_test;
mod roster_view;
#[cfg(test)]
mod roster_view_test;
mod screen;
mod status_line;
#[cfg(test)]
mod status_line_test;
mod tui_app;
mod ui_state;
#[cfg(test)]
mod ui_state_test;

pub use conversation_view::{AuthorRun, ConversationView, Entry, delivery_mark, delivery_text};
pub use key_binding::{KeyBindings, UiAction};
pub use peer_label::PeerLabels;
pub use roster_view::{RosterRow, roster_rows};
pub use status_line::StatusLine;
pub use tui_app::run;
pub use ui_state::{ConversationEntry, Mode, Overlay, UiState};
