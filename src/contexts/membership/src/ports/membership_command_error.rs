use std::fmt;

use crate::domain::PeerRosterError;
use crate::ports::{EventPublisherError, PeerTransportError};

/// Why a `membership` command could not be carried out.
///
/// Three sources, kept apart because they mean three different things to a
/// caller:
///
/// * [`Roster`](Self::Roster) — the domain refused. The request contradicted
///   what this peer knows (an unknown peer, a second session in one direction,
///   a session claiming the local peer's own identity). Retrying changes
///   nothing; the caller's view is wrong.
/// * [`Transport`](Self::Transport) — the network refused. Nothing was
///   recorded, and the same request may well succeed later.
/// * [`Publisher`](Self::Publisher) — the change *happened* but its
///   announcement did not. This is the one variant that leaves the roster
///   ahead of its consumers, which is why it is not folded into either of the
///   others.
///
/// The peer cache is absent on purpose: no command here fails because of it.
/// A cache that cannot be read costs a rung of the bootstrap ladder and is
/// reported in the join diagnostic; a cache that cannot be written costs a
/// warm start and is reported in the leave outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipCommandError {
    /// The roster rejected the transition.
    Roster(PeerRosterError),
    /// The transport could not carry it out.
    Transport(PeerTransportError),
    /// The change was made but could not be announced.
    Publisher(EventPublisherError),
}

impl fmt::Display for MembershipCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Roster(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
            Self::Publisher(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MembershipCommandError {}

impl From<PeerRosterError> for MembershipCommandError {
    fn from(error: PeerRosterError) -> Self {
        Self::Roster(error)
    }
}

impl From<PeerTransportError> for MembershipCommandError {
    fn from(error: PeerTransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<EventPublisherError> for MembershipCommandError {
    fn from(error: EventPublisherError) -> Self {
        Self::Publisher(error)
    }
}
