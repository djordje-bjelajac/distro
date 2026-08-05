use std::fmt;

/// Why a 1:1 message could not be delivered (D10, AC11).
///
/// Every failure names a cause. AC11 is explicit that silent loss is not a
/// state, and a bare "failed" would be silent loss with extra steps: the user
/// cannot decide whether to resend, wait, or verify a fingerprint without
/// knowing which of these happened.
///
/// These are *outcomes*, not transport mechanics. `NoRelayAvailable` is the
/// only honest name for safeguard S7's known limit — two symmetric-NAT peers
/// with no publicly reachable peer online genuinely cannot connect — and the
/// interface must be able to say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryFailure {
    /// No path to the recipient existed at all; they may simply be offline.
    PeerUnreachable,
    /// A direct path failed and no peer was available to relay (S7, AC12).
    NoRelayAvailable,
    /// The session died before the message was handed over — for instance a
    /// `PeerDisconnected` arriving while the send was still pending (D10).
    SessionClosed,
    /// The bounded retry cycle ended without an acknowledgement (D10). The
    /// user may resend; this context never queues for later delivery, because
    /// store-and-forward is excluded from v1.
    RetriesExhausted,
    /// The local transport was not running, so nothing was ever attempted.
    TransportUnavailable,
}

impl DeliveryFailure {
    /// Every reason, for exhaustive tests and interface rendering tables.
    pub const ALL: [Self; 5] = [
        Self::PeerUnreachable,
        Self::NoRelayAvailable,
        Self::SessionClosed,
        Self::RetriesExhausted,
        Self::TransportUnavailable,
    ];
}

impl fmt::Display for DeliveryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeerUnreachable => f.write_str("the recipient could not be reached"),
            Self::NoRelayAvailable => f.write_str("no peer is available to relay to the recipient"),
            Self::SessionClosed => f.write_str("the session closed before the message was sent"),
            Self::RetriesExhausted => f.write_str("delivery was retried and never acknowledged"),
            Self::TransportUnavailable => f.write_str("the local transport is not running"),
        }
    }
}
