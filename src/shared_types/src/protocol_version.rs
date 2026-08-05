/// Wire protocol version carried by every [`Envelope`](crate::Envelope)
/// (safeguard S2). Peers upgrade independently — there is no coordinated
/// deploy — so every codec decides per envelope how to treat the sender's
/// version via [`Compatibility::evaluate`](crate::Compatibility::evaluate).
///
/// `major` changes break the wire format; `minor` changes are additive only
/// (new fields, new payload kinds) and must remain readable by older peers
/// of the same major version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    /// The protocol version this build speaks.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}
