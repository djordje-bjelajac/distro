use crate::ProtocolVersion;

/// Decision of the S2 wire-compatibility rule: what a peer does with an
/// envelope whose [`ProtocolVersion`] differs from its own. Every codec
/// obeys this rule; it lives here as a pure function so all contexts and
/// adapters share one definition (AC14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compatibility {
    /// Same major, received minor is the same or lower: process normally.
    Accept,
    /// Same major, received minor is higher: the sender is newer. Process,
    /// but unknown fields and unknown payload kinds must be ignored and
    /// counted in local diagnostics — never treated as errors.
    Tolerate,
    /// Different major: incompatible wire format. Reject the envelope with
    /// a logged reason.
    Reject,
}

impl Compatibility {
    /// Evaluates a `received` envelope version against the version this
    /// build `supported`s. Pure function of its inputs.
    pub const fn evaluate(received: ProtocolVersion, supported: ProtocolVersion) -> Self {
        if received.major != supported.major {
            Self::Reject
        } else if received.minor <= supported.minor {
            Self::Accept
        } else {
            Self::Tolerate
        }
    }
}
