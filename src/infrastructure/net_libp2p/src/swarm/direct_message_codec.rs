use std::io;

use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::StreamProtocol;
use libp2p::request_response::Codec;

/// The 1:1 message protocol of D4, framed on the wire.
///
/// # Why a hand-written codec and not `request_response::cbor`
///
/// The ready-made CBOR codec deserializes first and bounds afterwards. S6 and
/// invariant 12 require the opposite order: an oversize frame must be refused
/// *before* deserialization, from its length prefix alone, so a stranger cannot
/// make this process allocate 400 MiB by claiming it is about to send that
/// much. Four bytes of length, checked against the cap, and only then a read of
/// exactly that many bytes.
///
/// # What travels in it
///
/// Opaque envelope bytes. This codec does not know what an
/// [`Envelope`](shared_types::Envelope) is and never decodes one — that is
/// [`EnvelopeCodec`](crate::codec::EnvelopeCodec), one layer up, so the size
/// gate and the content parser are separate things that fail separately.
///
/// The response is a single byte: whether the recipient took the message in.
/// It is not a read receipt and not an application acknowledgement of ordering
/// — it is what lets a `Pending` direct message become `Delivered` rather than
/// hanging (AC11).
#[derive(Debug, Clone)]
pub(crate) struct DirectMessageCodec {
    max_frame_bytes: usize,
}

/// The recipient's answer to one direct message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectMessageAck {
    /// Taken in. The sender may mark the message delivered.
    Accepted,
    /// Refused at this boundary — over the rate limit, oversize, or not a
    /// decodable envelope. Never silent: the sender learns, and the refusal is
    /// counted here.
    Refused,
}

impl DirectMessageAck {
    const ACCEPTED: u8 = 0;
    const REFUSED: u8 = 1;

    const fn code(self) -> u8 {
        match self {
            Self::Accepted => Self::ACCEPTED,
            Self::Refused => Self::REFUSED,
        }
    }

    /// Anything that is not the accepted code counts as a refusal, so a future
    /// peer that invents a new refusal reason does not read as success here.
    const fn from_code(code: u8) -> Self {
        if code == Self::ACCEPTED {
            Self::Accepted
        } else {
            Self::Refused
        }
    }
}

impl DirectMessageCodec {
    /// The protocol name. Versioned in the path so a future framing change is
    /// a new protocol older peers simply do not negotiate, rather than a
    /// misread frame.
    pub(crate) const PROTOCOL: StreamProtocol = StreamProtocol::new("/distro/direct/1.0.0");

    /// Bytes of length prefix ahead of every frame.
    const LENGTH_PREFIX_BYTES: usize = 4;

    pub(crate) const fn new(max_frame_bytes: usize) -> Self {
        Self { max_frame_bytes }
    }

    /// Reads one length-prefixed frame, refusing an oversize one before a
    /// single body byte is read (S6).
    async fn read_frame<T>(&self, io: &mut T) -> io::Result<Vec<u8>>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut prefix = [0_u8; Self::LENGTH_PREFIX_BYTES];
        io.read_exact(&mut prefix).await?;

        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.max_frame_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "direct message frame claims {length} bytes, over the {}-byte cap",
                    self.max_frame_bytes
                ),
            ));
        }

        // Allocation happens only after the claim has been bounded.
        let mut frame = vec![0_u8; length];
        io.read_exact(&mut frame).await?;

        Ok(frame)
    }

    async fn write_frame<T>(&self, io: &mut T, frame: &[u8]) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        if frame.len() > self.max_frame_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "direct message frame is {} bytes, over the {}-byte cap",
                    frame.len(),
                    self.max_frame_bytes
                ),
            ));
        }

        // The cast cannot lose information: the length is already bounded by
        // `max_frame_bytes`, which is far below `u32::MAX`.
        io.write_all(&(frame.len() as u32).to_be_bytes()).await?;
        io.write_all(frame).await
    }
}

#[async_trait]
impl Codec for DirectMessageCodec {
    type Protocol = StreamProtocol;
    type Request = Vec<u8>;
    type Response = DirectMessageAck;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        self.read_frame(io).await
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut code = [0_u8; 1];
        io.read_exact(&mut code).await?;

        Ok(DirectMessageAck::from_code(code[0]))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        request: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        self.write_frame(io, &request).await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        response: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&[response.code()]).await
    }
}
