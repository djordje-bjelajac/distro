use std::io::{self, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Somewhere text can be put for the user to paste elsewhere.
///
/// # A port on the root, not on a context
///
/// It carries no domain rule. A clipboard is a *device the terminal root
/// drives*, like the terminal itself, so it belongs here rather than in
/// `membership`'s or `messaging`'s `ports/` — a context that learned what a
/// clipboard is would have gained a dependency on how it is being looked at.
///
/// It keeps the `Port` suffix anyway, because that is what this workspace
/// calls a trait an adapter implements, and pretending otherwise would make it
/// look like something else.
///
/// # It must not become shared
///
/// When a desktop root arrives it will want a clipboard too, and the tempting
/// move is to lift this into a crate both can use. That crate would be a
/// composition root everything depends on, which the target layout forbids by
/// name. A desktop clipboard is also a *different device*: it answers, it can
/// fail in ways this one cannot report, and it has no terminal to write an
/// escape sequence to. Two implementations of two different things is the
/// honest arrangement.
pub trait ClipboardPort {
    /// Offers `text` to the clipboard.
    ///
    /// **Offers**, not "copies". See [`TerminalClipboard`] for why the weaker
    /// verb is the accurate one, and why every caller must keep it weak when
    /// it tells the user what happened.
    fn offer(&self, text: &str) -> io::Result<()>;
}

/// The clipboard reached through the terminal itself, by OSC 52.
///
/// # How it works
///
/// One escape sequence — `ESC ] 52 ; c ; <base64> BEL` — written to the
/// terminal, which is expected to put the decoded bytes on the system
/// clipboard. `c` is the selection: the clipboard proper, rather than the X11
/// primary selection that a middle-click would paste.
///
/// The point of doing it this way is that it works where a terminal
/// application actually runs. A native clipboard library links the window
/// system's libraries and therefore fails over SSH and in a headless session —
/// exactly where a peer-to-peer node is most likely to be running. The
/// terminal, by contrast, is by definition present, and tmux and most SSH
/// clients forward this sequence to the terminal the human is really sitting
/// at.
///
/// # It cannot be confirmed, and callers must not pretend otherwise
///
/// OSC 52 defines no reply. A terminal that has it disabled — several do by
/// default, on the reasonable grounds that a remote program writing to your
/// clipboard is a capability worth withholding — ignores the sequence in
/// silence. So `Ok(())` here means *the bytes were written to the terminal*,
/// and nothing more. It is not evidence that anything was copied, and no
/// notice built on it may say "copied".
///
/// That is a constraint on wording, not a defect to engineer around: the
/// alternative is a program that claims an outcome it has no way to observe,
/// which is the class of lie this build exists to avoid.
pub struct TerminalClipboard;

impl TerminalClipboard {
    /// The escape sequence that would be written for `text`.
    ///
    /// Split out from the write so the encoding can be asserted without a
    /// terminal: this is the part that has to be exactly right, and a test
    /// that had to own a TTY to check it would not be run.
    pub fn sequence_for(text: &str) -> String {
        format!("\x1b]52;c;{}\x07", STANDARD.encode(text.as_bytes()))
    }
}

impl ClipboardPort for TerminalClipboard {
    fn offer(&self, text: &str) -> io::Result<()> {
        // Straight to stdout, which is the terminal `ratatui` is drawing on.
        // Flushed immediately: an escape sequence sitting in a buffer until the
        // next redraw would arrive after the notice claiming it was sent.
        let mut out = io::stdout().lock();
        out.write_all(Self::sequence_for(text).as_bytes())?;
        out.flush()
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::io;
    use std::sync::Mutex;

    use super::ClipboardPort;

    /// A clipboard that keeps what it was offered.
    #[derive(Default)]
    pub(crate) struct RecordingClipboard {
        offered: Mutex<Vec<String>>,
    }

    impl RecordingClipboard {
        pub(crate) fn offered(&self) -> Vec<String> {
            self.offered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl ClipboardPort for RecordingClipboard {
        fn offer(&self, text: &str) -> io::Result<()> {
            self.offered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(text.to_owned());
            Ok(())
        }
    }

    /// A clipboard that cannot be written to at all.
    ///
    /// The rarer of the two failures — the terminal *refusing* is silent and
    /// indistinguishable from success — but a closed stdout is real, and the
    /// user is owed a different sentence for it.
    pub(crate) struct UnwritableClipboard;

    impl ClipboardPort for UnwritableClipboard {
        fn offer(&self, _text: &str) -> io::Result<()> {
            Err(io::Error::other("stdout is closed"))
        }
    }
}
