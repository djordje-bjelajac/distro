use std::fmt;

use crate::domain::{MessageBody, MessageBodyError, Millis, SequenceNumber, SequenceNumberError};

/// What one text message occupies inside an
/// [`Envelope`](shared_types::Envelope)'s opaque `payload` field.
///
/// # Why this lives in `ports` and not `domain` or an adapter
///
/// The domain models conversations and ordering and has no idea anything is
/// ever encoded — the same reason [`UnsignedEnvelope`](crate::ports::UnsignedEnvelope)
/// is here rather than there. It is not in an adapter either: the envelope
/// contract in `shared_types` deliberately keeps `payload` opaque so *each
/// context interprets it via its own codec*, and S2 (architect Note 4) pins the
/// envelope's signable layout per major version and states that additive minor
/// evolution "must ride inside `payload`". The shape of what rides inside is
/// therefore this context's contract, and every adapter that carries these
/// bytes carries them unread.
///
/// # Layout (a wire contract — a change here is a protocol change)
///
/// All integers big-endian, fields concatenated in order:
///
/// | offset | size | field                             |
/// |--------|------|-----------------------------------|
/// | 0      | 8    | `sequence` (`u64`, never zero)    |
/// | 8      | 8    | `claimed_sent_at` millis (`u64`)  |
/// | 16     | 4    | body length in bytes (`u32`)      |
/// | 20     | n    | body bytes, UTF-8                 |
///
/// Bytes **past** the body are ignored rather than refused. That is S2's
/// tolerance rule made concrete: peers upgrade independently and there is no
/// coordinated deploy, so a same-major peer that appends a field this build
/// does not know must still be readable. The length prefix is what makes that
/// safe — the decoder never reads past what arrived, so a hostile length is a
/// refusal and not an allocation (S6).
///
/// # No dependency, on purpose
///
/// Hand-written rather than CBOR (D6) because this crate takes no codec
/// dependency: `ciborium` lives in `infra-net-libp2p`, which encodes the
/// *envelope*. Doing it by hand here costs one pinned test and keeps the
/// application layer free of any serialization machinery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePayload {
    sequence: SequenceNumber,
    claimed_sent_at: Millis,
    body: MessageBody,
}

impl MessagePayload {
    /// Bytes before the body: the two `u64` fields and the body's length
    /// prefix.
    pub const HEADER_BYTES: usize = 8 + 8 + 4;

    pub const fn new(sequence: SequenceNumber, claimed_sent_at: Millis, body: MessageBody) -> Self {
        Self {
            sequence,
            claimed_sent_at,
            body,
        }
    }

    pub const fn sequence(&self) -> SequenceNumber {
        self.sequence
    }

    /// The author's claimed send time — display only, never an ordering or
    /// ageing input (invariant 5, rule R).
    pub const fn claimed_sent_at(&self) -> Millis {
        self.claimed_sent_at
    }

    pub const fn body(&self) -> &MessageBody {
        &self.body
    }

    /// Splits the payload into what
    /// [`Conversation::accept_remote`](crate::domain::Conversation::accept_remote)
    /// takes, so a handler never has to clone the body to use it.
    pub fn into_parts(self) -> (SequenceNumber, Millis, MessageBody) {
        (self.sequence, self.claimed_sent_at, self.body)
    }

    /// The bytes an [`Envelope`](shared_types::Envelope) carries.
    pub fn encode(&self) -> Vec<u8> {
        let body = self.body.as_str().as_bytes();
        let mut bytes = Vec::with_capacity(Self::HEADER_BYTES + body.len());

        bytes.extend_from_slice(&self.sequence.as_u64().to_be_bytes());
        bytes.extend_from_slice(&self.claimed_sent_at.as_millis().to_be_bytes());
        // The cast cannot lose information: `MessageBody` caps its length at
        // `MessageBody::MAX_BYTES` (16 KiB), far below `u32::MAX`.
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(body);

        bytes
    }

    /// Reads a payload out of the bytes an envelope carried.
    ///
    /// Every failure is a typed refusal. Nothing here panics, unwraps a slice,
    /// or allocates on a claimed length: hostile and truncated input is the
    /// normal case on an open network, and the caller turns any refusal into
    /// [`RejectionReason::MalformedPayload`](crate::domain::events::RejectionReason::MalformedPayload).
    pub fn decode(bytes: &[u8]) -> Result<Self, MessagePayloadError> {
        let header = bytes
            .get(..Self::HEADER_BYTES)
            .ok_or(MessagePayloadError::TooShort)?;

        let sequence = SequenceNumber::new(u64::from_be_bytes(
            header[..8].try_into().expect("eight bytes"),
        ))?;
        let claimed_sent_at = Millis::from_millis(u64::from_be_bytes(
            header[8..16].try_into().expect("eight bytes"),
        ));
        let length = u32::from_be_bytes(header[16..].try_into().expect("four bytes")) as usize;

        let end = Self::HEADER_BYTES
            .checked_add(length)
            .ok_or(MessagePayloadError::BodyTruncated)?;
        let body = bytes
            .get(Self::HEADER_BYTES..end)
            .ok_or(MessagePayloadError::BodyTruncated)?;

        let text = std::str::from_utf8(body).map_err(|_| MessagePayloadError::BodyNotUtf8)?;

        Ok(Self {
            sequence,
            claimed_sent_at,
            body: MessageBody::new(text)?,
        })
    }
}

/// Why a payload could not be read.
///
/// Deliberately distinct variants even though every one of them becomes the
/// same [`RejectionReason`](crate::domain::events::RejectionReason) — a local
/// diagnostic that cannot say whether a peer is sending truncated frames or
/// invalid text is a diagnostic that cannot find the bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePayloadError {
    /// Fewer bytes arrived than the fixed header needs.
    TooShort,
    /// The sequence number is not a valid one — zero, in practice, which is
    /// reserved so an absent mark can never be mistaken for a first message.
    InvalidSequence(SequenceNumberError),
    /// The body's length prefix names more bytes than arrived.
    BodyTruncated,
    /// The body's bytes are not UTF-8, so they are not text.
    BodyNotUtf8,
    /// The text is not an admissible body: empty, blank, or over the cap.
    InvalidBody(MessageBodyError),
}

impl From<SequenceNumberError> for MessagePayloadError {
    fn from(error: SequenceNumberError) -> Self {
        Self::InvalidSequence(error)
    }
}

impl From<MessageBodyError> for MessagePayloadError {
    fn from(error: MessageBodyError) -> Self {
        Self::InvalidBody(error)
    }
}

impl fmt::Display for MessagePayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => f.write_str("the payload is shorter than its header"),
            Self::InvalidSequence(error) => write!(f, "{error}"),
            Self::BodyTruncated => f.write_str("the payload body is shorter than it claims"),
            Self::BodyNotUtf8 => f.write_str("the payload body is not valid UTF-8"),
            Self::InvalidBody(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MessagePayloadError {}
