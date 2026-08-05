/// What one directed link between two simulated peers does to traffic.
///
/// # Directed, because real failures are
///
/// A NAT that admits inbound traffic in one direction only, an asymmetric
/// route, a firewall rule on one side — none of these are symmetric, and a
/// simulated network that could only model symmetric faults would be unable to
/// stage half the conditions D2's connectivity work exists to handle. Every
/// policy is therefore stored per ordered pair; the harness's symmetric helpers
/// set both directions and say so.
///
/// # Delay is a link property; ordering is a script
///
/// The delay here is the ordinary latency of a link. Deliberately scrambling
/// the order of a specific run of messages is a *script* on the fabric instead,
/// because a scenario that wants "these three arrive backwards" is describing
/// those three messages and not the link they travelled over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkPolicy {
    /// Milliseconds between a frame being handed to the transport and becoming
    /// due for delivery. Zero means it is due immediately — still only
    /// delivered by an explicit pump.
    pub delay: u64,
    /// The link carries nothing at all, in this direction.
    ///
    /// Distinct from a partition: a severed link is one broken path in an
    /// otherwise intact network, which is exactly the condition a *peer* relay
    /// exists to route around (AC12, D4). A partition splits the network and no
    /// relay can bridge it.
    pub severed: bool,
    /// Extra copies of each message frame, on top of the original.
    ///
    /// At-least-once delivery is the real network's guarantee; AC7 asks that
    /// exactly-once *application* hold over it, so a scenario must be able to
    /// make the same message arrive twice by the same path.
    pub duplicates: u8,
    /// What a dial along this link does when it is not simply severed.
    pub dial_fault: DialFault,
}

impl LinkPolicy {
    /// A perfect link: instant, lossless, single-copy.
    pub const PERFECT: Self = Self {
        delay: 0,
        severed: false,
        duplicates: 0,
        dial_fault: DialFault::None,
    };

    /// The same policy with a different delay.
    pub const fn delayed_by(mut self, millis: u64) -> Self {
        self.delay = millis;
        self
    }
}

/// How a dial along one link fails, when it fails.
///
/// The two failures are kept apart because `PeerTransportPort` keeps them
/// apart, and for the same reason: "nothing answered" and "something answered
/// but the authenticated handshake did not complete" call for different words
/// to a user and point at different faults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DialFault {
    /// Dials succeed.
    #[default]
    None,
    /// Nothing answers — the honest shape of S7's known limit.
    Unreachable,
    /// An endpoint answers and the handshake does not complete.
    HandshakeFailure,
}
