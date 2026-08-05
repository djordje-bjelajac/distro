use std::fmt;

use membership::domain::JoinTicketError;

use crate::mapping::EndpointMappingError;

/// Why pasted text is not a join ticket.
///
/// A ticket arrives by whatever channel two humans already share, so it gets
/// truncated by a chat client, re-wrapped by an email program, and pasted with
/// half the string missing. Every one of those is an ordinary occurrence with a
/// different answer for the user — "that is not a ticket", "that got cut off",
/// "that one has expired" — so the variants stay distinct rather than
/// collapsing into one unhelpful refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinTicketCodecError {
    /// The text is longer than a ticket may be (S6), refused before decoding.
    TooLarge { bytes: usize, limit: usize },
    /// The text does not start with the ticket prefix, so it is not a ticket
    /// of an encoding version this build knows.
    MissingPrefix,
    /// The body is not valid base64url — the usual shape of a truncated or
    /// re-wrapped paste.
    NotBase64,
    /// The body decoded but is not well-formed CBOR.
    MalformedCbor,
    /// The body is CBOR but not a map of named fields.
    NotAMap,
    /// A field this build requires is absent.
    MissingField(&'static str),
    /// A field is present with the wrong CBOR type.
    FieldType(&'static str),
    /// A field's value is outside the range its type allows.
    FieldRange(&'static str),
    /// The issuer field is not a valid Ed25519 public key.
    InvalidIssuer,
    /// One of the ticket's endpoints is not a dialable address.
    Endpoint(EndpointMappingError),
    /// The parts are all readable but the domain refuses the ticket they make.
    Rejected(JoinTicketError),
}

impl fmt::Display for JoinTicketCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, limit } => write!(
                f,
                "join ticket text is {bytes} bytes, refused before decoding at the {limit}-byte cap"
            ),
            Self::MissingPrefix => f.write_str(
                "text does not start with a join-ticket prefix this build knows how to read",
            ),
            Self::NotBase64 => {
                f.write_str("join ticket body is not valid base64url; it may have been truncated")
            }
            Self::MalformedCbor => f.write_str("join ticket body is not well-formed CBOR"),
            Self::NotAMap => f.write_str("join ticket body is CBOR but not a map of named fields"),
            Self::MissingField(name) => write!(f, "join ticket has no `{name}` field"),
            Self::FieldType(name) => {
                write!(f, "join ticket field `{name}` has the wrong CBOR type")
            }
            Self::FieldRange(name) => write!(f, "join ticket field `{name}` is out of range"),
            Self::InvalidIssuer => {
                f.write_str("join ticket issuer is not a valid Ed25519 public key")
            }
            Self::Endpoint(error) => write!(f, "join ticket endpoint is unusable: {error}"),
            Self::Rejected(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for JoinTicketCodecError {}
