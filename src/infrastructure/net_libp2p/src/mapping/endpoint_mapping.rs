use std::fmt;

use libp2p::Multiaddr;
use libp2p::multiaddr::Protocol;
use membership::domain::{Endpoint, EndpointError, Reachability};

/// Translates between the domain's opaque [`Endpoint`] and a libp2p
/// [`Multiaddr`].
///
/// # Where the reachability class comes from
///
/// It is **derived from the address**, never carried alongside it. An address
/// containing `/p2p-circuit` is one a third peer is relaying (AC12); anything
/// else is dialled directly. Deriving rather than trusting means a peer cannot
/// announce a circuit address labelled `Direct` and have the roster, the UI, and
/// S7's "no relay available" diagnostic all disagree with the wire.
///
/// # The domain never parses an address, and this is why
///
/// `Endpoint` validates only what holds for any textual address — non-empty,
/// bounded, no control characters — because the address grammar belongs to the
/// transport that produced it (canvas §2.2). Structural validation is S3's
/// "validate at the adapter", and it happens here: an address that is not a
/// well-formed multiaddress never becomes an `Endpoint` at all.
pub struct EndpointMapping;

impl EndpointMapping {
    /// The multiaddress an [`Endpoint`] denotes.
    pub fn to_multiaddr(endpoint: &Endpoint) -> Result<Multiaddr, EndpointMappingError> {
        endpoint
            .address()
            .parse()
            .map_err(|_| EndpointMappingError::MalformedAddress)
    }

    /// The [`Endpoint`] for a multiaddress, classified by what the address
    /// itself says.
    ///
    /// The empty multiaddress is refused. It parses — `""` is a syntactically
    /// valid `Multiaddr` with no components — but it names nothing to dial, and
    /// an announcement carrying it would put an undialable entry in every
    /// roster that heard it.
    pub fn to_endpoint(address: &Multiaddr) -> Result<Endpoint, EndpointMappingError> {
        if address.is_empty() {
            return Err(EndpointMappingError::MalformedAddress);
        }

        Endpoint::new(&address.to_string(), Self::reachability_of(address))
            .map_err(EndpointMappingError::Rejected)
    }

    /// Parses a multiaddress out of text and classifies it in one step, which
    /// is what a join ticket and a peer-cache line both need.
    pub fn parse(address: &str) -> Result<Endpoint, EndpointMappingError> {
        let parsed: Multiaddr = address
            .trim()
            .parse()
            .map_err(|_| EndpointMappingError::MalformedAddress)?;

        Self::to_endpoint(&parsed)
    }

    /// How an address is reached: through a peer relay when it contains a
    /// circuit hop, directly otherwise.
    pub fn reachability_of(address: &Multiaddr) -> Reachability {
        if address
            .iter()
            .any(|protocol| matches!(protocol, Protocol::P2pCircuit))
        {
            Reachability::Relayed
        } else {
            Reachability::Direct
        }
    }
}

/// Why an address could not be carried across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointMappingError {
    /// The text is not a well-formed multiaddress. Refused here, per S3, so
    /// the roster never holds an address nothing can dial.
    MalformedAddress,
    /// The address is well-formed but the domain refuses it — too long, or
    /// carrying a control character.
    Rejected(EndpointError),
}

impl fmt::Display for EndpointMappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedAddress => f.write_str("address is not a well-formed multiaddress"),
            Self::Rejected(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EndpointMappingError {}
