use std::fmt;

use shared_types::PeerId;

use crate::domain::Endpoint;

/// Opening, accepting, and closing authenticated links to peers (canvas §4).
///
/// **Transport level only.** Nothing here knows what a message is: `messaging`
/// addresses peers by `PeerId` through its own `MessageTransportPort`, and if
/// this trait grew a "send bytes to peer" method the two contexts would be
/// coupled through it. The split is what keeps `messaging` from ever learning
/// what an [`Endpoint`] is (canvas §4).
///
/// Inbound sessions do not appear here either: they arrive at the application
/// through `InboundSessionPort` (OP-6), because a port this layer *calls*
/// cannot also be how it is called.
pub trait PeerTransportPort {
    /// Starts accepting inbound sessions and reports the endpoints at which
    /// this peer is reachable.
    ///
    /// The result is what [`PeerDiscoveryPort::announce`](crate::ports::PeerDiscoveryPort::announce)
    /// publishes, so it must be endpoints *others* can dial rather than
    /// whatever was bound locally. Implementations return at least one
    /// endpoint or fail.
    fn listen(&self) -> Result<Vec<Endpoint>, PeerTransportError>;

    /// Dials `peer` at `endpoints`, in order, and reports the one that
    /// answered.
    ///
    /// Which endpoint answered is domain-relevant, not a detail: a relayed
    /// endpoint means a third peer is carrying the traffic (AC12), which the UI
    /// must be able to show and which S7 requires be stateable when it is
    /// unavailable.
    fn dial(&self, peer: PeerId, endpoints: &[Endpoint]) -> Result<Endpoint, PeerTransportError>;

    /// Closes the transport link to `peer`.
    ///
    /// Called for an ordinary close and for the session the collapse rule
    /// discarded (invariant 3) — the roster decides which, this executes it.
    fn close_session(&self, peer: PeerId) -> Result<(), PeerTransportError>;
}

/// Typed failure of a [`PeerTransportPort`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTransportError {
    /// The transport is not running.
    Unavailable,
    /// No listening endpoint could be established.
    ListenFailed,
    /// Every endpoint was tried and none answered.
    ///
    /// The honest name for S7's known limit: two symmetric-NAT peers with no
    /// relaying peer available simply cannot connect, and the UI must be able
    /// to say so rather than retry forever.
    NoReachableEndpoint,
    /// An endpoint answered but the authenticated handshake did not complete.
    HandshakeFailed,
    /// The transport holds no link to that peer.
    NoSuchSession,
}

impl fmt::Display for PeerTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the peer transport is not available"),
            Self::ListenFailed => f.write_str("the peer transport could not start listening"),
            Self::NoReachableEndpoint => f.write_str("no endpoint of the peer could be reached"),
            Self::HandshakeFailed => f.write_str("the session handshake with the peer failed"),
            Self::NoSuchSession => f.write_str("the transport holds no session for the peer"),
        }
    }
}

impl std::error::Error for PeerTransportError {}
