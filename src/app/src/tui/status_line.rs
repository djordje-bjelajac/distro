use infra_net_libp2p::swarm::Reachability;
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
///
/// # Two halves of one question
///
/// "Is this thing working" has two answers on a peer-to-peer network, and the
/// count answers only the outbound half. A peer can hold three sessions it
/// dialled and still be undialable itself, which is the single most confusing
/// failure in the product ("why can nobody reach me"). So reachability is shown
/// beside the count rather than instead of it (reachability canvas D6) — and
/// while nothing conclusive is known, it is shown as nothing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// `isolated`, `joining`, or `connected (n peers)`.
    pub network: String,
    /// Whether strangers can dial this peer — and **empty whenever the answer
    /// is not yet known**, which is most of a normal startup. See
    /// [`reachability_text`].
    pub reachability: String,
    /// This peer, by its own fingerprint.
    pub identity: String,
    /// The conversation the message pane is showing.
    pub conversation: String,
}

impl StatusLine {
    /// Builds the line.
    pub fn build(
        status: NetworkStatus,
        reachability: &Reachability,
        local: shared_types::PeerId,
        display_name: &str,
        conversation: &str,
    ) -> Self {
        Self {
            network: status.to_string(),
            reachability: reachability_text(reachability),
            identity: format!("{display_name} · {}", PeerLabels::short(local)),
            conversation: conversation.to_owned(),
        }
    }

    /// The whole line, for a pane that draws it as text.
    ///
    /// An unknown verdict takes its separator with it: a line reading
    /// `connected (3 peers) · │ …` would be a widget announcing that it has
    /// nothing to say, which is the spinner S3 forbids wearing a different hat.
    pub fn text(&self) -> String {
        let network = if self.reachability.is_empty() {
            self.network.clone()
        } else {
            format!("{} · {}", self.network, self.reachability)
        };

        format!("{network} │ {} │ {}", self.identity, self.conversation)
    }

    /// Whether this instance is reaching nobody — the one state S7 says the
    /// interface must be able to state plainly.
    pub fn is_isolated(&self) -> bool {
        self.network == NetworkStatus::Isolated.to_string()
    }
}

/// What the status line says about reachability — and what it deliberately does
/// not say.
///
/// # `Unknown` is nothing at all
///
/// Not a spinner, not `checking…`, not a warning, not a dimmed placeholder.
/// During normal startup every peer is `Unknown`, so anything rendered here is
/// rendered by every instance on every launch — and anything that reads like a
/// verdict is exactly the false negative the whole piece exists to prevent
/// (reachability canvas D6, S3). The absence is the design, not an omission.
///
/// # `Reachable` names the address
///
/// "You are reachable" without saying where is not something a user can check,
/// paste into a message, or compare against the ticket they just handed out. A
/// relayed path is called out separately because reachable through someone
/// else's bandwidth is a different fact from reachable directly, and the
/// address alone does not read as either.
///
/// # `Unreachable` states a consequence, never a cause
///
/// A corroborated failure means strangers' dials are not arriving. It does not
/// say why, and this process cannot know: a NAT, a firewall this user does not
/// administer, a carrier-grade translation layer, and a network that simply
/// carries no inbound connections are indistinguishable from here. So the
/// wording names what follows — a relay will be needed — and issues no
/// instruction, because every instruction available ("forward a port", "change
/// a router setting") is one this user may have no way to carry out, and the
/// evidence behind the verdict is corroborated hearsay rather than proof.
fn reachability_text(reachability: &Reachability) -> String {
    match reachability {
        Reachability::Unknown => String::new(),
        Reachability::Reachable(endpoint) if endpoint.is_relayed() => {
            format!("reachable through a relay at {endpoint}")
        }
        Reachability::Reachable(endpoint) => format!("reachable at {endpoint}"),
        Reachability::Unreachable => {
            "not reachable from outside — a relay will be needed".to_owned()
        }
    }
}
