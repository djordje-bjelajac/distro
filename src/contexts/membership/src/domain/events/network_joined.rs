use std::num::NonZeroUsize;

use crate::domain::Millis;

/// The local peer reached the network: at least one session is established
/// after a join attempt (canvas §2.2).
///
/// Emitted by the `JoinNetwork` command (OP-6) once the bootstrap ladder of D1
/// — cached peers, then LAN, then a pasted ticket — produced a live link. A
/// ladder that produces none leaves the peer `Isolated`, which is a normal
/// state and not an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkJoined {
    pub at: Millis,
    /// Peers with an established session at the moment of joining; never zero,
    /// because zero is precisely the case where no join happened.
    pub connected_peers: NonZeroUsize,
}
