use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ciborium::value::Value;
use membership::domain::{JoinTicket, Millis};
use shared_types::{PeerId, ProtocolVersion};

use crate::limits::ResourceLimits;
use crate::mapping::EndpointMapping;
use crate::ticket::JoinTicketCodecError;

/// The copy-pasteable string form of a [`JoinTicket`] (D1).
///
/// # Why this exists at all
///
/// Internet-wide first contact needs a first peer, and every automatic way of
/// supplying one — hardcoded bootstrap hosts, public rendezvous, DNS seeds — is
/// operator-run infrastructure S1 forbids. A ticket moves that first contact to
/// a human channel the participants already have: one string, pasted once per
/// machine, and never needed again on it (AC3).
///
/// # Encoding only
///
/// Whether a ticket *may be redeemed* is [`JoinTicket::validate`] — a pure
/// domain rule over the ticket, a clock reading, and the version this build
/// speaks. Nothing here re-checks expiry or protocol compatibility: putting the
/// rule on both sides of the boundary is how a clock on one side and a clock on
/// the other come to disagree. This module turns a ticket into text and text
/// back into a ticket, and refuses text that is not one.
///
/// # Self-describing, and versioned separately from the protocol
///
/// The string is `distro-join-1.<base64url>`. The `1` is the *encoding*
/// version, not the protocol version: it says how to read the bytes, while the
/// protocol version rides inside them and is the domain's business. A future
/// encoding change bumps the prefix and old software says "I do not know this
/// format" instead of misreading it.
///
/// The body is CBOR with named fields (D6), tolerant of fields it does not
/// know, so a newer issuer can add one without making its tickets unreadable to
/// everyone who has not upgraded.
pub struct JoinTicketCodec;

/// The human-readable prefix and encoding version.
///
/// A prefix rather than a bare blob so a user who pastes it somewhere wrong,
/// or a support conversation about it, can tell at a glance what the string is.
const PREFIX: &str = "distro-join-1.";

const FIELD_ISSUER: &str = "issuer";
const FIELD_ENDPOINTS: &str = "endpoints";
const FIELD_PROTOCOL_MAJOR: &str = "protocol_major";
const FIELD_PROTOCOL_MINOR: &str = "protocol_minor";
const FIELD_EXPIRES_AT_MILLIS: &str = "expires_at_millis";

impl JoinTicketCodec {
    /// The prefix every ticket string carries.
    pub const PREFIX: &'static str = PREFIX;

    /// Renders `ticket` as one line a human can paste.
    ///
    /// # A note on the expiry for the composition root
    ///
    /// [`Millis`] is documented as a reading on *this peer's* monotonic
    /// timeline with an unspecified origin, and a ticket carries that reading
    /// across to a machine that will compare it against its own clock. For the
    /// comparison to mean anything, both peers' `ClockPort` must read from a
    /// shared origin — UNIX-epoch milliseconds is the obvious choice, and it is
    /// the composition root's decision to make (OP-12), not this codec's. The
    /// codec carries the number the domain gave it, unaltered.
    pub fn encode(ticket: &JoinTicket) -> String {
        let record = Value::Map(vec![
            (
                Value::Text(FIELD_ISSUER.to_owned()),
                Value::Bytes(ticket.issuer().as_bytes().to_vec()),
            ),
            (
                Value::Text(FIELD_ENDPOINTS.to_owned()),
                Value::Array(
                    ticket
                        .endpoints()
                        .iter()
                        .map(|endpoint| Value::Text(endpoint.address().to_owned()))
                        .collect(),
                ),
            ),
            (
                Value::Text(FIELD_PROTOCOL_MAJOR.to_owned()),
                Value::Integer(ticket.protocol().major.into()),
            ),
            (
                Value::Text(FIELD_PROTOCOL_MINOR.to_owned()),
                Value::Integer(ticket.protocol().minor.into()),
            ),
            (
                Value::Text(FIELD_EXPIRES_AT_MILLIS.to_owned()),
                Value::Integer(ticket.expires_at().as_millis().into()),
            ),
        ]);

        let mut body = Vec::new();
        // Infallible in practice: the value tree above is built from owned,
        // already-valid data and the writer is a `Vec`. An empty body decodes
        // to `MalformedCbor` rather than producing a ticket, so even the
        // impossible branch fails closed.
        let _ = ciborium::into_writer(&record, &mut body);

        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(&body))
    }

    /// Reads a ticket out of pasted text.
    ///
    /// Every failure is a typed refusal. Nothing here panics, indexes a slice,
    /// or allocates on a claimed length: a ticket arrives from a chat message
    /// and is exactly the kind of input that gets truncated, re-wrapped, and
    /// tampered with.
    ///
    /// The **reachability class of each endpoint is derived from its address**
    /// rather than read from the ticket, so an issuer cannot label a circuit
    /// address `Direct` and have the roster disagree with the wire (AC12).
    pub fn decode(text: &str) -> Result<JoinTicket, JoinTicketCodecError> {
        Self::decode_within(text, ResourceLimits::DEFAULT)
    }

    /// [`decode`](Self::decode) with an explicit size budget, so a test can
    /// exercise the cap without building a 4 KiB string.
    pub fn decode_within(
        text: &str,
        limits: ResourceLimits,
    ) -> Result<JoinTicket, JoinTicketCodecError> {
        let trimmed = text.trim();

        if trimmed.len() > limits.max_ticket_bytes {
            return Err(JoinTicketCodecError::TooLarge {
                bytes: trimmed.len(),
                limit: limits.max_ticket_bytes,
            });
        }

        let encoded = trimmed
            .strip_prefix(PREFIX)
            .ok_or(JoinTicketCodecError::MissingPrefix)?;
        let body = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| JoinTicketCodecError::NotBase64)?;

        let value: Value = ciborium::from_reader(body.as_slice())
            .map_err(|_| JoinTicketCodecError::MalformedCbor)?;
        let Value::Map(entries) = value else {
            return Err(JoinTicketCodecError::NotAMap);
        };

        let issuer_bytes: [u8; PeerId::LENGTH] = read_bytes(&entries, FIELD_ISSUER)?
            .try_into()
            .map_err(|_| JoinTicketCodecError::InvalidIssuer)?;
        let issuer = PeerId::from_public_key_bytes(issuer_bytes)
            .map_err(|_| JoinTicketCodecError::InvalidIssuer)?;

        let addresses = read_array(&entries, FIELD_ENDPOINTS)?;
        let mut endpoints = Vec::with_capacity(addresses.len());
        for address in addresses {
            let Value::Text(text) = address else {
                return Err(JoinTicketCodecError::FieldType(FIELD_ENDPOINTS));
            };
            endpoints.push(EndpointMapping::parse(&text).map_err(JoinTicketCodecError::Endpoint)?);
        }

        let protocol = ProtocolVersion::new(
            read_u16(&entries, FIELD_PROTOCOL_MAJOR)?,
            read_u16(&entries, FIELD_PROTOCOL_MINOR)?,
        );
        let expires_at = Millis::from_millis(read_u64(&entries, FIELD_EXPIRES_AT_MILLIS)?);

        // The one construction rule — at least one endpoint — belongs to the
        // domain, so it is the domain that enforces it.
        JoinTicket::new(issuer, endpoints, protocol, expires_at)
            .map_err(JoinTicketCodecError::Rejected)
    }
}

fn field<'a>(entries: &'a [(Value, Value)], name: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(key, _)| matches!(key, Value::Text(text) if text == name))
        .map(|(_, value)| value)
}

fn read_bytes(
    entries: &[(Value, Value)],
    name: &'static str,
) -> Result<Vec<u8>, JoinTicketCodecError> {
    match field(entries, name).ok_or(JoinTicketCodecError::MissingField(name))? {
        Value::Bytes(bytes) => Ok(bytes.clone()),
        _ => Err(JoinTicketCodecError::FieldType(name)),
    }
}

fn read_array(
    entries: &[(Value, Value)],
    name: &'static str,
) -> Result<Vec<Value>, JoinTicketCodecError> {
    match field(entries, name).ok_or(JoinTicketCodecError::MissingField(name))? {
        Value::Array(items) => Ok(items.clone()),
        _ => Err(JoinTicketCodecError::FieldType(name)),
    }
}

fn read_u64(entries: &[(Value, Value)], name: &'static str) -> Result<u64, JoinTicketCodecError> {
    let Value::Integer(integer) =
        field(entries, name).ok_or(JoinTicketCodecError::MissingField(name))?
    else {
        return Err(JoinTicketCodecError::FieldType(name));
    };

    u128::try_from(*integer)
        .ok()
        .and_then(|wide| u64::try_from(wide).ok())
        .ok_or(JoinTicketCodecError::FieldRange(name))
}

fn read_u16(entries: &[(Value, Value)], name: &'static str) -> Result<u16, JoinTicketCodecError> {
    u16::try_from(read_u64(entries, name)?).map_err(|_| JoinTicketCodecError::FieldRange(name))
}
