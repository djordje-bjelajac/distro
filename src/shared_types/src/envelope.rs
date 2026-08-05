use crate::{Compatibility, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

/// The wire unit every message travels in: a versioned, signed wrapper
/// around an opaque payload (canvas §7/S2, invariant 4).
///
/// This is a plain struct. Wire encoding/decoding lives in adapter codecs;
/// signing and verification live behind identity-context ports. The payload
/// stays opaque bytes here — each context interprets it via its own codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub version: ProtocolVersion,
    pub kind: PayloadKind,
    pub author: PeerId,
    pub payload: Vec<u8>,
    pub signature: EnvelopeSignature,
}

impl Envelope {
    /// The canonical byte string the author signs and every receiver
    /// verifies. Codec-independent by design: whatever encoding an adapter
    /// uses on the wire, the signing input is this exact layout, so peers
    /// with different codec versions still verify each other's signatures.
    ///
    /// # Layout (stable — pinned by test; a change requires a major
    /// protocol version bump)
    ///
    /// All integers big-endian, fields concatenated in order:
    ///
    /// | offset | size | field                                    |
    /// |--------|------|------------------------------------------|
    /// | 0      | 2    | `version.major` (`u16`)                  |
    /// | 2      | 2    | `version.minor` (`u16`)                  |
    /// | 4      | 2    | `kind` wire code (`u16`)                 |
    /// | 6      | 4    | author key length in bytes (`u32`, = 32) |
    /// | 10     | 32   | author public-key bytes                  |
    /// | 42     | 4    | payload length in bytes (`u32`)          |
    /// | 46     | n    | payload bytes                            |
    ///
    /// The signature itself is **not** covered. Length prefixes make the
    /// encoding injective: no two distinct field combinations produce the
    /// same byte string.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let author_bytes = self.author.as_bytes();
        let mut bytes =
            Vec::with_capacity(2 + 2 + 2 + 4 + author_bytes.len() + 4 + self.payload.len());
        bytes.extend_from_slice(&self.version.major.to_be_bytes());
        bytes.extend_from_slice(&self.version.minor.to_be_bytes());
        bytes.extend_from_slice(&self.kind.code().to_be_bytes());
        bytes.extend_from_slice(&(author_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(author_bytes);
        bytes.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Applies the S2 compatibility rule to this envelope's version against
    /// the version this build supports. See [`Compatibility::evaluate`].
    pub const fn compatibility(&self, supported: &ProtocolVersion) -> Compatibility {
        Compatibility::evaluate(self.version, *supported)
    }
}
