use std::fmt;
use std::num::NonZeroUsize;

use crate::domain::PeerStanding;

/// How connected this peer currently is to the network (canvas §2.2).
///
/// `Isolated` is a **normal state, not an error**: a fresh install with no
/// cached peers, no LAN neighbour, and no pasted ticket is isolated by
/// definition (D1), and so is a laptop that has just woken up. The value exists
/// so the UI can say so plainly (S7) instead of a failure being reported where
/// there is none.
///
/// `Connected` carries a [`NonZeroUsize`], which makes "connected to zero
/// peers" unrepresentable — that state has a name of its own, and having two
/// encodings for it is exactly how a status line ends up lying.
///
/// # It counts sessions, not live peers
///
/// The question this value answers is "can I do anything right now", so it
/// counts peers a direct message can be sent to. Counting peers that are *live
/// by evidence* instead would report `connected (5)` where zero directs can be
/// sent — the mirror of the lie it would be fixing — and would silently
/// redefine `Isolated`, which is already the session predicate (canvas D4).
/// Coherence with the roster comes from [`PeerStanding`] instead: see
/// [`from_standings`](Self::from_standings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkStatus {
    /// No established session to any peer.
    Isolated,
    /// A join attempt is in flight: the bootstrap ladder of D1 is being walked.
    Joining,
    /// Established sessions to this many peers.
    Connected(NonZeroUsize),
}

impl NetworkStatus {
    /// Classifies a roster by counting its [`Linked`](PeerStanding::Linked)
    /// standings.
    ///
    /// This is the coherent entry point (canvas D5). The status line and the
    /// roster rows are computed from **one** slice of standings, so the number
    /// in `connected (n)` is by construction the number of rows that render as
    /// linked. The arithmetic is identical to
    /// [`from_connected_peers`](Self::from_connected_peers) and identical to
    /// what
    /// [`PeerRoster::established_session_count`](crate::domain::PeerRoster::established_session_count)
    /// already produced — the fix is not a better count. It is that there is no
    /// longer a second traversal with a second predicate for the two to
    /// disagree about, which is how `connected (2 peers)` came to sit above a
    /// roster of `offline` rows.
    ///
    /// A linked peer counts whatever its presence.
    /// [`Linked(Offline)`](PeerStanding::Linked) is a working link to a peer
    /// that is not answering; dropping it from the count to make the screen
    /// agree with itself would hide a link a direct message can still be
    /// attempted over (safeguard S4).
    pub fn from_standings(standings: &[PeerStanding]) -> Self {
        Self::from_connected_peers(
            standings
                .iter()
                .filter(|standing| standing.is_linked())
                .count(),
        )
    }

    /// Classifies a count of peers with established sessions.
    ///
    /// Zero collapses to [`Isolated`](Self::Isolated). `Joining` is never
    /// derived here: it describes an in-flight operation, which the count alone
    /// cannot distinguish from having simply not started.
    ///
    /// The shared arithmetic beneath [`from_standings`](Self::from_standings).
    /// Callers with a roster in hand want that one: a bare count has already
    /// discarded which peers it counted, and it is precisely that discarding
    /// that let the status line and the rows be derived independently.
    pub const fn from_connected_peers(count: usize) -> Self {
        match NonZeroUsize::new(count) {
            Some(peers) => Self::Connected(peers),
            None => Self::Isolated,
        }
    }

    /// Peers with an established session; zero unless [`Connected`](Self::Connected).
    pub const fn connected_peers(self) -> usize {
        match self {
            Self::Connected(peers) => peers.get(),
            Self::Isolated | Self::Joining => 0,
        }
    }

    pub const fn is_isolated(self) -> bool {
        matches!(self, Self::Isolated)
    }
}

impl fmt::Display for NetworkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Isolated => f.write_str("isolated"),
            Self::Joining => f.write_str("joining"),
            Self::Connected(peers) if peers.get() == 1 => f.write_str("connected (1 peer)"),
            Self::Connected(peers) => write!(f, "connected ({peers} peers)"),
        }
    }
}
