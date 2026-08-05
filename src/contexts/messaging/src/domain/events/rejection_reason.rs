use std::fmt;

/// Why inbound content never became a message (S3, AC6, AC15).
///
/// The vocabulary lives in the domain because it is the context's own account
/// of what it refuses, but only
/// [`ArrivedAfterGapClosed`](Self::ArrivedAfterGapClosed) is produced *by* the
/// domain — the rest are decided at the application boundary, before an
/// aggregate is ever touched, which is the whole point of S3: adapters never
/// construct domain aggregates from raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectionReason {
    /// The signature did not verify against the claimed author, so there is no
    /// author at all (invariant 4). Content like this never reaches a read
    /// model (invariant 10).
    SignatureInvalid,
    /// The author is on this peer's local block list (invariant 11). Blocking
    /// is purely local; nothing is announced to anyone.
    AuthorBlocked,
    /// The envelope's major protocol version is one this build cannot read
    /// (S2, AC14).
    UnsupportedProtocolVersion,
    /// The payload did not decode into a message this context understands.
    MalformedPayload,
    /// The message's place in its author's run had already been given up on:
    /// the gap it belonged to was abandoned — see
    /// [`MessageGapClosed`](crate::domain::events::MessageGapClosed) — and the
    /// log has moved past it.
    ///
    /// This is **not** a duplicate, and calling it one is the mistake the
    /// variant exists to prevent: a duplicate means "you already have this",
    /// this means "you will never have this". Invariant 6 draws the line
    /// exactly there, and AC15 requires the loss to be reported rather than
    /// swallowed.
    ArrivedAfterGapClosed,
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignatureInvalid => {
                f.write_str("the envelope signature did not verify against its author")
            }
            Self::AuthorBlocked => f.write_str("the author is blocked locally"),
            Self::UnsupportedProtocolVersion => {
                f.write_str("the envelope's protocol major version is not supported")
            }
            Self::MalformedPayload => f.write_str("the payload is not a readable message"),
            Self::ArrivedAfterGapClosed => {
                f.write_str("the message arrived after its gap had been abandoned")
            }
        }
    }
}
