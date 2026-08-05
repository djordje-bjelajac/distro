/// The kind of payload an [`Envelope`](crate::Envelope) carries, identified
/// on the wire by a `u16` code.
///
/// The code assignments in [`code`](Self::code) are a compatibility contract
/// (pinned by test): existing codes are never renumbered or reused, new kinds
/// take fresh codes in a minor version bump.
///
/// S2 tolerance: codecs map an unassigned code to [`Unknown`](Self::Unknown)
/// instead of failing, so newer peers can ship new kinds without breaking
/// older ones. `Unknown` retains the original code, so re-encoding forwards
/// it unchanged. `Unknown` is for *unassigned* codes only — constructing
/// `Unknown(c)` with an assigned `c` is a programming error; use
/// [`from_code`](Self::from_code), which never does so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PayloadKind {
    /// A 1:1 message to a single peer.
    DirectMessage,
    /// A message on the network-wide broadcast channel.
    BroadcastMessage,
    /// A liveness signal feeding presence derivation.
    Heartbeat,
    /// A kind this build does not know; carries the received wire code.
    Unknown(u16),
}

impl PayloadKind {
    /// The wire code for this kind.
    pub const fn code(&self) -> u16 {
        match self {
            Self::DirectMessage => 0,
            Self::BroadcastMessage => 1,
            Self::Heartbeat => 2,
            Self::Unknown(code) => *code,
        }
    }

    /// Maps a wire code to its kind; unassigned codes yield `Unknown(code)`.
    pub const fn from_code(code: u16) -> Self {
        match code {
            0 => Self::DirectMessage,
            1 => Self::BroadcastMessage,
            2 => Self::Heartbeat,
            other => Self::Unknown(other),
        }
    }
}
