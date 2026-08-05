use std::fmt;
use std::num::NonZeroUsize;

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
    /// Classifies a count of peers with established sessions.
    ///
    /// Zero collapses to [`Isolated`](Self::Isolated). `Joining` is never
    /// derived here: it describes an in-flight operation, which the count alone
    /// cannot distinguish from having simply not started.
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
