use std::fmt;

use shared_types::ProtocolVersion;

/// Why a frame off the wire is not an [`Envelope`](shared_types::Envelope).
///
/// Every variant carries enough to be *the logged reason* S2 demands for a
/// rejection. Distinct variants for what could be one "malformed" because a
/// diagnostic that cannot tell an incompatible peer from a truncated frame from
/// a forged key is a diagnostic that cannot find the problem — and on a network
/// with no operator, this process's own log is the only place an answer lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeCodecError {
    /// The frame exceeds the S6 cap. Detected from the byte count alone,
    /// before anything is deserialized (invariant 12).
    TooLarge { bytes: usize, limit: usize },
    /// The bytes are not well-formed CBOR.
    MalformedCbor,
    /// The bytes decoded, but not to a map of named fields (D6).
    NotAMap,
    /// A field this build requires is absent.
    MissingField(&'static str),
    /// A field is present with the wrong CBOR type.
    FieldType(&'static str),
    /// A field's value is outside the range its type allows.
    FieldRange(&'static str),
    /// The S2 rejection: a different major version is an incompatible wire
    /// format, and no amount of tolerance makes it readable.
    IncompatibleMajor {
        received: ProtocolVersion,
        supported: ProtocolVersion,
    },
    /// The author field is not a valid Ed25519 public key, so the envelope
    /// names no identity (invariant 1).
    InvalidAuthor,
    /// The signature field is not 64 bytes, so it is not an Ed25519 signature.
    InvalidSignature,
    /// The payload alone exceeds what an envelope may carry.
    PayloadTooLarge { bytes: usize, limit: usize },
    /// The envelope could not be written out.
    EncodeFailed,
}

impl fmt::Display for EnvelopeCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, limit } => write!(
                f,
                "envelope frame is {bytes} bytes, refused before decoding at the {limit}-byte cap"
            ),
            Self::MalformedCbor => f.write_str("envelope frame is not well-formed CBOR"),
            Self::NotAMap => f.write_str("envelope frame is CBOR but not a map of named fields"),
            Self::MissingField(name) => write!(f, "envelope frame has no `{name}` field"),
            Self::FieldType(name) => write!(f, "envelope field `{name}` has the wrong CBOR type"),
            Self::FieldRange(name) => write!(f, "envelope field `{name}` is out of range"),
            Self::IncompatibleMajor {
                received,
                supported,
            } => write!(
                f,
                "envelope speaks protocol {}.{} and this build speaks {}.{}; \
                 a different major version is an incompatible wire format",
                received.major, received.minor, supported.major, supported.minor
            ),
            Self::InvalidAuthor => f.write_str("envelope author is not a valid Ed25519 public key"),
            Self::InvalidSignature => f.write_str("envelope signature is not 64 bytes"),
            Self::PayloadTooLarge { bytes, limit } => write!(
                f,
                "envelope payload is {bytes} bytes, over the {limit}-byte cap"
            ),
            Self::EncodeFailed => f.write_str("envelope could not be encoded"),
        }
    }
}

impl std::error::Error for EnvelopeCodecError {}
