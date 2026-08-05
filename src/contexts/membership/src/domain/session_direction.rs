use std::fmt;

use shared_types::PeerId;

/// Which side dialled, seen from the local peer (canvas §2.2).
///
/// Direction is not bookkeeping: it is the whole input to the session-collapse
/// rule (invariant 3). The rule names a *peer* — the lexicographically lower
/// one — as the initiator whose session survives, and direction is how each
/// side translates that shared answer into "the session I opened" or "the
/// session they opened", with no message exchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionDirection {
    /// The remote peer dialled us.
    Inbound,
    /// We dialled the remote peer.
    Outbound,
}

impl SessionDirection {
    /// The peer that initiated a session with this direction.
    pub const fn initiator(self, local: PeerId, remote: PeerId) -> PeerId {
        match self {
            Self::Inbound => remote,
            Self::Outbound => local,
        }
    }

    /// The same wire session as the other side names it.
    pub const fn opposite(self) -> Self {
        match self {
            Self::Inbound => Self::Outbound,
            Self::Outbound => Self::Inbound,
        }
    }
}

impl fmt::Display for SessionDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbound => f.write_str("inbound"),
            Self::Outbound => f.write_str("outbound"),
        }
    }
}
