use std::fmt;

use libp2p::identity::Keypair;
use shared_types::PeerId;

use crate::mapping::{PeerIdMapping, PeerIdMappingError};

/// The keypair the swarm authenticates every connection with.
///
/// # Why key material has to come in here at all
///
/// The transport handshake — Noise on TCP and relayed circuits, TLS 1.3 on QUIC
/// — proves that the peer on the other end holds the private key behind its
/// `PeerId`. That proof is what makes a session *authenticated* rather than
/// merely encrypted, and it cannot be delegated across a port: the handshake
/// happens inside the swarm, on bytes the swarm owns.
///
/// So this crate holds the key, and holds it only here. It is never logged
/// (see the redacting [`Debug`] below), never returned by any method, never
/// written to disk by this crate, and never sent over any channel. The identity
/// context still owns *creation and persistence* of the key (D5); the
/// composition root loads it and hands it to this constructor once at startup.
/// That is the one crossing, and it is a deliberate one — flagged for OP-12
/// rather than smuggled.
#[derive(Clone)]
pub struct NetworkIdentity {
    keypair: Keypair,
    peer: PeerId,
}

impl NetworkIdentity {
    /// Builds an identity from a 32-byte Ed25519 secret key.
    ///
    /// The caller's buffer is **zeroed** as a side effect: `libp2p` clears the
    /// slice it is given, and this method deliberately does not copy it first.
    /// One fewer copy of a secret in memory is worth the surprising signature.
    pub fn from_ed25519_secret_key(secret: &mut [u8; 32]) -> Result<Self, NetworkIdentityError> {
        let keypair =
            Keypair::ed25519_from_bytes(secret).map_err(|_| NetworkIdentityError::MalformedKey)?;
        let peer = PeerIdMapping::from_libp2p(&keypair.public().to_peer_id())
            .map_err(NetworkIdentityError::Mapping)?;

        Ok(Self { keypair, peer })
    }

    /// The identity this key denotes, in the domain's terms.
    pub const fn peer_id(&self) -> PeerId {
        self.peer
    }

    pub(crate) const fn keypair(&self) -> &Keypair {
        &self.keypair
    }
}

impl fmt::Debug for NetworkIdentity {
    /// Prints the public identity and nothing else.
    ///
    /// A derived `Debug` would put the private key in every error message,
    /// panic backtrace, and log line that ever formatted a config — which is
    /// exactly how secrets escape.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkIdentity")
            .field("peer_id", &self.peer)
            .field("secret_key", &"<redacted>")
            .finish()
    }
}

/// Why an identity could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkIdentityError {
    /// The bytes are not a valid Ed25519 secret key.
    MalformedKey,
    /// The derived public key could not be expressed as a domain `PeerId`.
    Mapping(PeerIdMappingError),
}

impl fmt::Display for NetworkIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedKey => f.write_str("bytes are not a valid Ed25519 secret key"),
            Self::Mapping(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for NetworkIdentityError {}
