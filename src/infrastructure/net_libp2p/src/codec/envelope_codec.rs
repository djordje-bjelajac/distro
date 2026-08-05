use ciborium::value::Value;
use shared_types::{
    Compatibility, Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion,
};

use crate::codec::{CodecDiagnostics, EnvelopeCodecError};
use crate::limits::ResourceLimits;

/// The wire encoding of an [`Envelope`]: CBOR with named fields, tolerant of
/// fields it does not know (D6).
///
/// # Why named fields and not a positional encoding
///
/// Peers upgrade independently and there is no coordinated deploy, ever (S2).
/// A positional format — `postcard`, `bincode` — makes any field addition a
/// break for every peer that has not upgraded, which on this network means
/// "half of them, indefinitely". Named fields are what make S2's rule
/// *implementable* rather than aspirational: an older peer can skip a field it
/// has no name for and still read the rest.
///
/// # This codec never reconstructs the signing input
///
/// [`Envelope::signable_bytes`] is a fixed layout in `shared_types`, pinned per
/// major version, precisely so that peers running different codecs still verify
/// each other's signatures. If this codec built the signing input from its own
/// field order, a codec change would silently invalidate every signature on the
/// network. So it does not: it carries the signature as an opaque 64-byte
/// field and never touches it. Verification happens above this line, against
/// `Envelope::signable_bytes()`, through the identity context's verifier port.
///
/// # The S2 rule is borrowed, never re-derived
///
/// Compatibility is decided by [`Compatibility::evaluate`] and payload kinds by
/// [`PayloadKind::from_code`] — both in `shared_types`, both pure. A second
/// copy of either rule here is how two peers of the same build come to disagree
/// about the same envelope.
#[derive(Debug, Clone)]
pub struct EnvelopeCodec {
    supported: ProtocolVersion,
    limits: ResourceLimits,
    diagnostics: CodecDiagnostics,
}

/// Field names on the wire. Adding a name here is a minor protocol change;
/// renaming or removing one is a major change (S2).
const FIELD_VERSION_MAJOR: &str = "version_major";
const FIELD_VERSION_MINOR: &str = "version_minor";
const FIELD_KIND: &str = "kind";
const FIELD_AUTHOR: &str = "author";
const FIELD_PAYLOAD: &str = "payload";
const FIELD_SIGNATURE: &str = "signature";

/// Every field this build has a name for. Anything else on the wire is
/// counted and ignored (S2).
const KNOWN_FIELDS: [&str; 6] = [
    FIELD_VERSION_MAJOR,
    FIELD_VERSION_MINOR,
    FIELD_KIND,
    FIELD_AUTHOR,
    FIELD_PAYLOAD,
    FIELD_SIGNATURE,
];

impl EnvelopeCodec {
    /// A codec speaking `supported`, bounded by `limits`, counting into
    /// `diagnostics`.
    pub fn new(
        supported: ProtocolVersion,
        limits: ResourceLimits,
        diagnostics: CodecDiagnostics,
    ) -> Self {
        Self {
            supported,
            limits,
            diagnostics,
        }
    }

    /// The protocol version this codec speaks.
    pub const fn supported(&self) -> ProtocolVersion {
        self.supported
    }

    /// The counters this codec writes into.
    pub fn diagnostics(&self) -> &CodecDiagnostics {
        &self.diagnostics
    }

    /// The largest frame this codec will look at.
    pub const fn max_frame_bytes(&self) -> usize {
        self.limits.max_envelope_bytes
    }

    /// Writes `envelope` out as a CBOR map.
    ///
    /// An unknown [`PayloadKind`] is written back with the code it arrived
    /// with, so forwarding an envelope this build does not understand does not
    /// mangle it.
    pub fn encode(&self, envelope: &Envelope) -> Result<Vec<u8>, EnvelopeCodecError> {
        if envelope.payload.len() > self.limits.max_envelope_bytes {
            return Err(EnvelopeCodecError::PayloadTooLarge {
                bytes: envelope.payload.len(),
                limit: self.limits.max_envelope_bytes,
            });
        }

        let record = Value::Map(vec![
            (
                Value::Text(FIELD_VERSION_MAJOR.to_owned()),
                Value::Integer(envelope.version.major.into()),
            ),
            (
                Value::Text(FIELD_VERSION_MINOR.to_owned()),
                Value::Integer(envelope.version.minor.into()),
            ),
            (
                Value::Text(FIELD_KIND.to_owned()),
                Value::Integer(envelope.kind.code().into()),
            ),
            (
                Value::Text(FIELD_AUTHOR.to_owned()),
                Value::Bytes(envelope.author.as_bytes().to_vec()),
            ),
            (
                Value::Text(FIELD_PAYLOAD.to_owned()),
                Value::Bytes(envelope.payload.clone()),
            ),
            (
                Value::Text(FIELD_SIGNATURE.to_owned()),
                Value::Bytes(envelope.signature.as_bytes().to_vec()),
            ),
        ]);

        let mut bytes = Vec::new();
        ciborium::into_writer(&record, &mut bytes).map_err(|_| EnvelopeCodecError::EncodeFailed)?;

        if bytes.len() > self.limits.max_envelope_bytes {
            return Err(EnvelopeCodecError::PayloadTooLarge {
                bytes: bytes.len(),
                limit: self.limits.max_envelope_bytes,
            });
        }

        Ok(bytes)
    }

    /// Reads an envelope out of a frame, applying the S2 rule.
    ///
    /// The size cap is checked **first**, from the byte count alone, so an
    /// oversize frame is refused without a single byte being deserialized
    /// (invariant 12, S6).
    pub fn decode(&self, frame: &[u8]) -> Result<Envelope, EnvelopeCodecError> {
        if frame.len() > self.limits.max_envelope_bytes {
            self.diagnostics.count_oversize_frame();
            return Err(EnvelopeCodecError::TooLarge {
                bytes: frame.len(),
                limit: self.limits.max_envelope_bytes,
            });
        }

        self.decode_within_cap(frame)
            .inspect_err(|error| match error {
                EnvelopeCodecError::IncompatibleMajor { .. } => {
                    self.diagnostics.count_rejected_major();
                }
                _ => self.diagnostics.count_malformed_frame(),
            })
    }

    fn decode_within_cap(&self, frame: &[u8]) -> Result<Envelope, EnvelopeCodecError> {
        let value: Value =
            ciborium::from_reader(frame).map_err(|_| EnvelopeCodecError::MalformedCbor)?;
        let Value::Map(entries) = value else {
            return Err(EnvelopeCodecError::NotAMap);
        };

        let version = ProtocolVersion::new(
            read_u16(&entries, FIELD_VERSION_MAJOR)?,
            read_u16(&entries, FIELD_VERSION_MINOR)?,
        );

        // The one S2 decision, taken by the one S2 function. `Reject` is a
        // *refusal with a reason*, which is what the error's `Display` is.
        match Compatibility::evaluate(version, self.supported) {
            Compatibility::Reject => {
                return Err(EnvelopeCodecError::IncompatibleMajor {
                    received: version,
                    supported: self.supported,
                });
            }
            Compatibility::Tolerate => self.diagnostics.count_tolerated_minor(),
            Compatibility::Accept => {}
        }

        // Same major: anything this build has no name for is ignored and
        // counted, never treated as an error.
        let unknown = entries
            .iter()
            .filter(|(key, _)| match key {
                Value::Text(name) => !KNOWN_FIELDS.contains(&name.as_str()),
                _ => true,
            })
            .count();
        if unknown > 0 {
            self.diagnostics.count_unknown_fields(unknown as u64);
        }

        let kind = PayloadKind::from_code(read_u16(&entries, FIELD_KIND)?);
        if matches!(kind, PayloadKind::Unknown(_)) {
            self.diagnostics.count_unknown_payload_kind();
        }

        let author_bytes: [u8; PeerId::LENGTH] = read_bytes(&entries, FIELD_AUTHOR)?
            .try_into()
            .map_err(|_| EnvelopeCodecError::InvalidAuthor)?;
        let author = PeerId::from_public_key_bytes(author_bytes)
            .map_err(|_| EnvelopeCodecError::InvalidAuthor)?;

        let signature_bytes: [u8; EnvelopeSignature::LENGTH] =
            read_bytes(&entries, FIELD_SIGNATURE)?
                .try_into()
                .map_err(|_| EnvelopeCodecError::InvalidSignature)?;

        let payload = read_bytes(&entries, FIELD_PAYLOAD)?;
        if payload.len() > self.limits.max_envelope_bytes {
            return Err(EnvelopeCodecError::PayloadTooLarge {
                bytes: payload.len(),
                limit: self.limits.max_envelope_bytes,
            });
        }

        Ok(Envelope {
            version,
            kind,
            author,
            payload,
            signature: EnvelopeSignature::new(signature_bytes),
        })
    }
}

/// Looks up a named field, returning `None` rather than the first match of a
/// non-text key.
fn field<'a>(entries: &'a [(Value, Value)], name: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(key, _)| matches!(key, Value::Text(text) if text == name))
        .map(|(_, value)| value)
}

fn read_u16(entries: &[(Value, Value)], name: &'static str) -> Result<u16, EnvelopeCodecError> {
    let value = field(entries, name).ok_or(EnvelopeCodecError::MissingField(name))?;
    let Value::Integer(integer) = value else {
        return Err(EnvelopeCodecError::FieldType(name));
    };

    u128::try_from(*integer)
        .ok()
        .and_then(|wide| u16::try_from(wide).ok())
        .ok_or(EnvelopeCodecError::FieldRange(name))
}

fn read_bytes(
    entries: &[(Value, Value)],
    name: &'static str,
) -> Result<Vec<u8>, EnvelopeCodecError> {
    let value = field(entries, name).ok_or(EnvelopeCodecError::MissingField(name))?;
    match value {
        Value::Bytes(bytes) => Ok(bytes.clone()),
        _ => Err(EnvelopeCodecError::FieldType(name)),
    }
}
