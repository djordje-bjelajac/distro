use membership::domain::LivenessWindows;
use shared_types::ProtocolVersion;

/// The constants every simulated peer's contexts are assembled with.
///
/// Grouped into one value rather than threaded through five constructors so a
/// scenario can vary exactly the one it is interrogating, and so adding a
/// setting does not change a signature every peer construction depends on.
///
/// Every field has a default that matches what a real launch would use, so a
/// scenario that overrides nothing is testing the shipped behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimSettings {
    /// The wire protocol every simulated peer speaks.
    ///
    /// Overriding it for one peer is how AC14's "an unsupported major version
    /// is rejected with a logged reason" becomes a two-peer scenario rather
    /// than a hand-built envelope.
    pub protocol: ProtocolVersion,
    /// The evidence-age thresholds presence is derived against (invariant 7).
    ///
    /// The defaults are the shipped ones — 30 s to `Stale`, 60 s to `Offline` —
    /// so an AC5 scenario advancing a minute of virtual time is testing the
    /// real windows. Shortening them is a convenience, never a requirement:
    /// virtual time is free.
    pub liveness_windows: LivenessWindows,
    /// How long a gap may stay open before the sweep gives up on it (rule R,
    /// S6). Defaults to `Conversation::GAP_TOLERANCE`.
    pub gap_tolerance: messaging::domain::DurationMillis,
    /// How many messages one peer's in-memory log holds before refusing to
    /// grow (D7, S6).
    pub message_log_capacity: usize,
    /// Whether a delivered 1:1 message produces an acknowledgement back to its
    /// sender, turning `Pending` into `Delivered` (AC11).
    ///
    /// On by default because that is what a real transport does. Turning it off
    /// is how a scenario holds a message at `Pending` to watch what a
    /// disconnect does to it (D10).
    pub acknowledge_directs: bool,
}

impl SimSettings {
    /// What a real launch uses.
    pub fn shipped() -> Self {
        Self {
            protocol: ProtocolVersion::CURRENT,
            liveness_windows: LivenessWindows::DEFAULT,
            gap_tolerance: messaging::domain::Conversation::GAP_TOLERANCE,
            message_log_capacity: crate::stores::InMemoryMessageLog::DEFAULT_CAPACITY,
            acknowledge_directs: true,
        }
    }
}

impl Default for SimSettings {
    fn default() -> Self {
        Self::shipped()
    }
}
