use std::fmt;

use crate::domain::JoinTicketError;
use crate::ports::{PeerCacheError, PeerDiscoveryError};

/// Why one rung of the bootstrap ladder produced no connection.
///
/// Every variant is a *normal* outcome — a fresh install has an empty cache, a
/// laptop on a café network has no LAN neighbour, and most launches carry no
/// ticket. None of them is an error in the `Result` sense, because the ladder
/// as a whole has a defined answer either way: `Isolated`, which is a state
/// and not a failure (canvas §2.2).
///
/// They are kept apart because they call for different words to the user, and
/// AC3 is satisfied by a diagnostic that *names what was tried and why it did
/// not work* — "no peer answered" and "the discovery service is not running"
/// are the same silence with completely different remedies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RungFailure {
    /// The rung produced nothing to dial: an empty cache, a quiet LAN, or no
    /// ticket supplied.
    NoCandidates,
    /// The peer cache could not be read. The warm start is lost; the remaining
    /// rungs are unaffected.
    Cache(PeerCacheError),
    /// The discovery mechanism refused or is not running.
    Discovery(PeerDiscoveryError),
    /// The ticket itself cannot be redeemed — expired, or a protocol major
    /// this build does not speak (S2, AC14). Checked before it is handed to
    /// the adapter, so an unusable ticket never reaches the network.
    Ticket(JoinTicketError),
    /// Candidates existed and every one of them was tried; none answered.
    ///
    /// The honest shape of S7's known limit: with no publicly reachable peer
    /// online, two symmetric-NAT peers simply cannot connect, and the UI must
    /// be able to say so rather than spin.
    Unreachable { candidates: usize },
}

impl fmt::Display for RungFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCandidates => f.write_str("nothing to try"),
            Self::Cache(error) => write!(f, "{error}"),
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Ticket(error) => write!(f, "{error}"),
            Self::Unreachable { candidates: 1 } => f.write_str("1 peer tried, none answered"),
            Self::Unreachable { candidates } => {
                write!(f, "{candidates} peers tried, none answered")
            }
        }
    }
}

impl From<PeerCacheError> for RungFailure {
    fn from(error: PeerCacheError) -> Self {
        Self::Cache(error)
    }
}

impl From<PeerDiscoveryError> for RungFailure {
    fn from(error: PeerDiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<JoinTicketError> for RungFailure {
    fn from(error: JoinTicketError) -> Self {
        Self::Ticket(error)
    }
}
