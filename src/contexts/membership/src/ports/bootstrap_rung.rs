use std::fmt;

/// One rung of the D1 bootstrap ladder, in the order `JoinNetwork` walks them.
///
/// The order is not arbitrary and is not a preference. Each rung costs the
/// user strictly more than the one above it:
///
/// 1. [`CachedPeers`](Self::CachedPeers) — free and silent. After one
///    successful join, this machine never needs another rung.
/// 2. [`LocalNetwork`](Self::LocalNetwork) — free and automatic, but only
///    reaches as far as the broadcast domain (AC2).
/// 3. [`JoinTicket`](Self::JoinTicket) — the only rung that costs a human
///    action, and the honest price of internet-wide first contact with no
///    operator-run infrastructure (S1). Reached only when the two free rungs
///    produced nothing.
///
/// There is deliberately no fourth rung. A hardcoded bootstrap host, a public
/// rendezvous, or a DNS seed would make first contact automatic and would make
/// every participant depend on something a non-participant operates — which is
/// the one requirement this whole design exists to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootstrapRung {
    /// Peers this machine already knew from a previous session.
    CachedPeers,
    /// Peers the discovery mechanism sees on its own — mDNS on the LAN, and
    /// whatever routing the adapter offers.
    LocalNetwork,
    /// The peer named by a join ticket pasted in out of band.
    JoinTicket,
}

impl BootstrapRung {
    /// Every rung, in ladder order.
    pub const LADDER: [Self; 3] = [Self::CachedPeers, Self::LocalNetwork, Self::JoinTicket];
}

impl fmt::Display for BootstrapRung {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CachedPeers => f.write_str("cached peers"),
            Self::LocalNetwork => f.write_str("local network"),
            Self::JoinTicket => f.write_str("join ticket"),
        }
    }
}
