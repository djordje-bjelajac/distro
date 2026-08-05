/// Who ended a session, which decides whether the transport still has to be
/// told.
///
/// The roster transition is identical either way — the session closes and, if
/// it had established, `PeerDisconnected` is published. What differs is the
/// one side effect: a local decision must reach the transport, while a link
/// the transport itself just reported dead does not need to be closed a second
/// time. Asking it to would be noise at best, and at worst an error a caller
/// would have to learn to ignore, which is how genuine errors get ignored too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionCloseCause {
    /// This peer decided to end the session — leaving the network, or a user
    /// disconnecting from a peer. The transport is asked to close the link.
    LocalDecision,
    /// The transport reported the link ended: the remote closed it, it timed
    /// out, or it failed. Nothing is asked of the transport.
    TransportReported,
}

impl SessionCloseCause {
    /// Whether the transport still holds a link that must be closed.
    pub const fn closes_the_transport_link(self) -> bool {
        matches!(self, Self::LocalDecision)
    }
}
