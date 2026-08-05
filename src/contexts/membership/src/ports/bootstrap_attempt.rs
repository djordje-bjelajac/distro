use std::fmt;

use shared_types::{Fingerprint, PeerId};

use crate::ports::{BootstrapRung, RungFailure};

/// What one rung of the bootstrap ladder actually did.
///
/// The outcome is a `Result` because the two cases are genuinely exclusive and
/// exhaustive: a rung either produced a peer this instance is now connected to,
/// or it produced a reason it did not. Modelling it as two optional fields
/// would admit a fourth state — connected *and* failed, or neither — that no
/// walk of the ladder can reach.
///
/// A successful rung names exactly one peer. The ladder stops at the first
/// connection because its job is *first contact*, not building a mesh: once one
/// peer answers, discovery and gossip supply the rest, and dialling every
/// cached peer on every launch would make a cold start cost proportional to
/// how long the machine has been a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapAttempt {
    /// Which rung this was.
    pub rung: BootstrapRung,
    /// The peer that answered, or why none did.
    pub outcome: Result<PeerId, RungFailure>,
}

impl BootstrapAttempt {
    /// Records a rung that connected `peer`.
    pub const fn connected(rung: BootstrapRung, peer: PeerId) -> Self {
        Self {
            rung,
            outcome: Ok(peer),
        }
    }

    /// Records a rung that produced no connection.
    pub const fn failed(rung: BootstrapRung, failure: RungFailure) -> Self {
        Self {
            rung,
            outcome: Err(failure),
        }
    }

    /// The peer this rung connected, if it connected one.
    pub const fn peer(&self) -> Option<PeerId> {
        match self.outcome {
            Ok(peer) => Some(peer),
            Err(_) => None,
        }
    }

    /// Why this rung produced nothing, if it produced nothing.
    pub const fn failure(&self) -> Option<RungFailure> {
        match self.outcome {
            Ok(_) => None,
            Err(failure) => Some(failure),
        }
    }

    pub const fn succeeded(&self) -> bool {
        self.outcome.is_ok()
    }
}

impl fmt::Display for BootstrapAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.outcome {
            Ok(peer) => write!(f, "{}: connected to {}", self.rung, Fingerprint::of(peer)),
            Err(failure) => write!(f, "{}: {failure}", self.rung),
        }
    }
}
