use membership::domain::NetworkStatus;

use crate::tui::PeerLabels;

/// The one-line summary at the top of the screen.
///
/// # `isolated` is not an error, and the wording matters
///
/// The canvas is explicit that `Isolated` is a normal state (§2.2, S7): a fresh
/// install on a quiet network with no ticket is *supposed* to reach it, and so
/// is a laptop that just woke up. So the status reads `isolated` and the log
/// pane carries the account of what was tried (AC3) — nothing here says
/// "error", "failed", or "disconnected", because none of those is what
/// happened.
///
/// `joining` is reported for exactly as long as a bootstrap ladder is in
/// flight, which no count of sessions could tell the caller — that is why
/// `NetworkStatus` carries it as a state of its own rather than deriving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// `isolated`, `joining`, or `connected (n peers)`.
    pub network: String,
    /// This peer, by its own fingerprint.
    pub identity: String,
    /// The conversation the message pane is showing.
    pub conversation: String,
}

impl StatusLine {
    /// Builds the line.
    pub fn build(
        status: NetworkStatus,
        local: shared_types::PeerId,
        display_name: &str,
        conversation: &str,
    ) -> Self {
        Self {
            network: status.to_string(),
            identity: format!("{display_name} · {}", PeerLabels::short(local)),
            conversation: conversation.to_owned(),
        }
    }

    /// The whole line, for a pane that draws it as text.
    pub fn text(&self) -> String {
        format!(
            "{} │ {} │ {}",
            self.network, self.identity, self.conversation
        )
    }

    /// Whether this instance is reaching nobody — the one state S7 says the
    /// interface must be able to state plainly.
    pub fn is_isolated(&self) -> bool {
        self.network == NetworkStatus::Isolated.to_string()
    }
}
