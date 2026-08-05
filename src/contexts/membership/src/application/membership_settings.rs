use shared_types::{PeerId, ProtocolVersion};

use crate::domain::LivenessWindows;

/// The three constants a `membership` context needs before it can decide
/// anything: who this peer is, what protocol this build speaks, and how long
/// silence may last before a peer is treated as gone.
///
/// They are grouped rather than passed as three loose arguments because they
/// share a lifetime — all three are fixed for the life of the process — and
/// because a constructor taking eight positional parameters invites two
/// `PeerId`-shaped arguments to be swapped without the compiler noticing.
///
/// None of them is a port: nothing here is fetched, read from disk, or
/// negotiated. The `PeerId` comes from `identity` through the composition root
/// (never by importing that context), and the other two are this build's own
/// facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembershipSettings {
    /// The peer this instance is. Every roster operation rejects it as a
    /// remote (invariant 2).
    pub local_peer: PeerId,
    /// The wire protocol this build speaks, used to judge a join ticket's
    /// compatibility (S2, AC14).
    pub protocol: ProtocolVersion,
    /// The evidence-age thresholds presence is derived against (invariant 7).
    pub liveness_windows: LivenessWindows,
}

impl MembershipSettings {
    /// The defaults every launch uses: this build's protocol version and the
    /// engineering liveness windows pinned in the domain.
    pub const fn for_local_peer(local_peer: PeerId) -> Self {
        Self {
            local_peer,
            protocol: ProtocolVersion::CURRENT,
            liveness_windows: LivenessWindows::DEFAULT,
        }
    }

    /// Overrides the protocol version — for a test pinning the compatibility
    /// rule, or a build that deliberately speaks an older major.
    pub const fn with_protocol(mut self, protocol: ProtocolVersion) -> Self {
        self.protocol = protocol;
        self
    }

    /// Overrides the liveness windows, so a deterministic scenario can expire
    /// a peer without advancing a fake clock by a minute.
    pub const fn with_liveness_windows(mut self, liveness_windows: LivenessWindows) -> Self {
        self.liveness_windows = liveness_windows;
        self
    }
}
