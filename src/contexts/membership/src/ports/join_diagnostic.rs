use std::fmt;

use shared_types::PeerId;

use crate::ports::{
    BootstrapAttempt, BootstrapRung, PeerDiscoveryError, PeerTransportError, RungFailure,
};

/// The record of one walk of the D1 bootstrap ladder: what was tried, in what
/// order, and what each attempt produced.
///
/// # This type is AC3
///
/// > *"failure produces a visible diagnostic, never a hang."*
///
/// A serverless join has no authority to ask why it failed, so the only honest
/// answer is a full account of what this peer itself attempted. Every rung
/// reached appears here with its own reason — an empty cache and a cache that
/// could not be read are different sentences, as are "no LAN neighbour" and
/// "the discovery service is not running" — and [`Display`](fmt::Display)
/// renders the whole thing for a status pane or a log line.
///
/// A ladder that ends in `Isolated` is therefore never silent, and never a
/// wait: every rung either answered or reported, and the walk is over when the
/// last rung is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinDiagnostic {
    /// One entry per rung actually walked, in ladder order. Short: the ladder
    /// stops at the first rung that connects.
    pub attempts: Vec<BootstrapAttempt>,
    /// The transport could not start listening, so this peer is dialling out
    /// while unreachable itself. Not fatal to a join — outbound links still
    /// work — but it is why nobody dials back, which a user otherwise has no
    /// way to discover.
    pub listen_failure: Option<PeerTransportError>,
    /// The local peer's addresses could not be announced, so others will not
    /// find it by discovery even though it may be perfectly reachable.
    pub announce_failure: Option<PeerDiscoveryError>,
}

impl JoinDiagnostic {
    /// The peer the ladder connected, if any rung did.
    pub fn connected_peer(&self) -> Option<PeerId> {
        self.attempts.iter().find_map(BootstrapAttempt::peer)
    }

    /// Whether the ladder reached the network.
    pub fn succeeded(&self) -> bool {
        self.connected_peer().is_some()
    }

    /// The rungs this walk actually reached, in order.
    pub fn rungs_tried(&self) -> Vec<BootstrapRung> {
        self.attempts.iter().map(|attempt| attempt.rung).collect()
    }

    /// Why one rung produced nothing, or `None` if it succeeded or was never
    /// reached.
    pub fn failure_of(&self, rung: BootstrapRung) -> Option<RungFailure> {
        self.attempts
            .iter()
            .find(|attempt| attempt.rung == rung)
            .and_then(BootstrapAttempt::failure)
    }

    pub(crate) fn record(&mut self, attempt: BootstrapAttempt) {
        self.attempts.push(attempt);
    }
}

impl fmt::Display for JoinDiagnostic {
    /// One headline plus one line per rung, so a status pane can show the
    /// first line and a log can keep all of them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.connected_peer() {
            Some(_) => f.write_str("joined the network")?,
            None => f.write_str("could not reach the network; every bootstrap path failed")?,
        }

        for attempt in &self.attempts {
            write!(f, "\n  {attempt}")?;
        }
        if let Some(error) = self.listen_failure {
            write!(f, "\n  not listening: {error}")?;
        }
        if let Some(error) = self.announce_failure {
            write!(f, "\n  not announced: {error}")?;
        }

        Ok(())
    }
}
