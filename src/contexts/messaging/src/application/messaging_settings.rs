use shared_types::{PeerId, ProtocolVersion};

use crate::domain::{Conversation, DurationMillis};

/// The constants one `messaging` context runs with.
///
/// Grouped into one value rather than passed as four constructor arguments so
/// a composition root cannot silently transpose two of them, and so a test can
/// vary exactly the one it is interrogating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagingSettings {
    /// The peer this instance is. Every message it composes is authored by
    /// this identity, and the signer holds the matching key.
    pub local_peer: PeerId,
    /// The protocol version this build speaks: stamped on every envelope it
    /// sends, and the yardstick every arriving envelope is measured against
    /// (S2, AC14).
    pub protocol_version: ProtocolVersion,
    /// How long a gap may stay open before the sweep gives up on it (rule R,
    /// S6).
    ///
    /// A setting rather than a constant because the aggregate takes it as a
    /// parameter — a deployment on a slower path may want longer, and a test
    /// wants to decide it outright.
    pub gap_tolerance: DurationMillis,
}

impl MessagingSettings {
    /// The defaults: the version this build speaks and the tolerance the
    /// aggregate documents.
    pub const fn for_local_peer(local_peer: PeerId) -> Self {
        Self {
            local_peer,
            protocol_version: ProtocolVersion::CURRENT,
            gap_tolerance: Conversation::GAP_TOLERANCE,
        }
    }

    /// The same settings with a different tolerance window.
    pub const fn with_gap_tolerance(self, gap_tolerance: DurationMillis) -> Self {
        Self {
            gap_tolerance,
            ..self
        }
    }

    /// The same settings speaking a different protocol version.
    pub const fn speaking(self, protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            ..self
        }
    }
}
