use std::fmt;

use crate::domain::Reachability;

/// One address at which a peer may be reachable, plus how it is reached
/// (canvas §2.2).
///
/// The address is an **opaque string** — a multiaddress as far as this context
/// is concerned, but the domain never parses it. Two reasons, both structural:
/// no `std::net` type may enter a domain module (canvas §4), and the address
/// syntax belongs to the transport that produced it, so parsing here would
/// couple the roster to today's libp2p address grammar (D2) and force a domain
/// change the day a transport gains a new component.
///
/// Validation is therefore only what holds for *any* textual address: it must
/// be non-empty, bounded, and free of control characters. A structurally
/// invalid multiaddress is rejected where it is understood — in the adapter,
/// per S3 — and simply never becomes an `Endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Endpoint {
    address: String,
    reachability: Reachability,
}

impl Endpoint {
    /// Longest address this context will hold, in bytes.
    ///
    /// A fully-qualified relayed multiaddress — transport address, `quic-v1`,
    /// the relay's base-58 peer id, `p2p-circuit`, the target's peer id — runs
    /// to roughly 160 bytes, so 256 leaves headroom for a longer future
    /// transport component while still bounding what one announcement can cost
    /// in memory and in cache-file size (S6 in spirit; the wire-level cap is
    /// enforced in the adapter).
    pub const MAX_ADDRESS_BYTES: usize = 256;

    /// Validates `address` and pairs it with its reachability class.
    ///
    /// Leading and trailing whitespace is trimmed first, so an address pasted
    /// out of a chat message or a join ticket is accepted in the form a human
    /// actually copies. Control characters are rejected anywhere in the
    /// trimmed text: endpoints are echoed into logs and into the roster pane of
    /// the TUI (D8), where an escape sequence could forge UI structure.
    pub fn new(address: &str, reachability: Reachability) -> Result<Self, EndpointError> {
        let trimmed = address.trim();

        if trimmed.is_empty() {
            return Err(EndpointError::Empty);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(EndpointError::ContainsControlCharacter);
        }
        if trimmed.len() > Self::MAX_ADDRESS_BYTES {
            return Err(EndpointError::TooLong {
                bytes: trimmed.len(),
                limit: Self::MAX_ADDRESS_BYTES,
            });
        }

        Ok(Self {
            address: trimmed.to_owned(),
            reachability,
        })
    }

    /// An endpoint dialled directly.
    pub fn direct(address: &str) -> Result<Self, EndpointError> {
        Self::new(address, Reachability::Direct)
    }

    /// An endpoint reached through a peer relay.
    pub fn relayed(address: &str) -> Result<Self, EndpointError> {
        Self::new(address, Reachability::Relayed)
    }

    /// The trimmed, validated address.
    pub fn address(&self) -> &str {
        &self.address
    }

    pub const fn reachability(&self) -> Reachability {
        self.reachability
    }

    pub const fn is_relayed(&self) -> bool {
        self.reachability.is_relayed()
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.address)
    }
}

/// Typed construction error for [`Endpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointError {
    /// The address is empty once surrounding whitespace is removed.
    Empty,
    /// The trimmed address contains a control character.
    ContainsControlCharacter,
    /// The trimmed address exceeds [`Endpoint::MAX_ADDRESS_BYTES`].
    TooLong {
        /// Bytes counted in the trimmed address.
        bytes: usize,
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("endpoint address is empty after trimming"),
            Self::ContainsControlCharacter => {
                f.write_str("endpoint address contains a control character")
            }
            Self::TooLong { bytes, limit } => {
                write!(f, "endpoint address is {bytes} bytes, limit is {limit}")
            }
        }
    }
}

impl std::error::Error for EndpointError {}
