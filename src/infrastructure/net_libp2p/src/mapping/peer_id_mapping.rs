use std::fmt;

use libp2p::identity::PublicKey;
use libp2p::identity::ed25519;
use shared_types::PeerId;

/// The identity hash code in the multicodec table.
///
/// A multihash with this code carries its input *verbatim* rather than a digest
/// of it. libp2p uses it for public keys short enough to inline — which is
/// every Ed25519 key, at 32 bytes plus a small protobuf wrapper, well under the
/// 42-byte threshold. That is what makes this mapping bidirectional: a libp2p
/// `PeerId` for an Ed25519 peer *contains* the key rather than a hash of it.
const IDENTITY_HASH_CODE: u64 = 0x00;

/// Translates between this network's identity and libp2p's.
///
/// # Why these are two different types at all
///
/// [`shared_types::PeerId`] is a raw 32-byte Ed25519 public key with one
/// invariant: it only exists for bytes that decode to a valid key. libp2p's
/// `PeerId` is a multihash over a protobuf-wrapped public key of any supported
/// algorithm. Neither can be the other without dragging its whole world along —
/// `shared_types` would gain a libp2p dependency, or the domain would gain a
/// hash it has no rule for — so the translation stops here (D2's containment
/// rule) and this is the only file in the workspace that knows both shapes.
///
/// # Nothing here panics
///
/// A remote peer chooses its own identity, so "a libp2p `PeerId` this build
/// cannot express" is ordinary inbound data on an open network, not a
/// programming error. Every conversion returns a typed refusal.
pub struct PeerIdMapping;

impl PeerIdMapping {
    /// The libp2p identity for a domain [`PeerId`].
    ///
    /// The error is unreachable given invariant 1 — a `PeerId` cannot hold
    /// bytes that are not a valid key — but it is returned rather than
    /// panicked. An adapter is the wrong place for a process to die, and an
    /// "impossible" branch that unwraps is how an invariant change three
    /// releases later becomes a crash instead of a compile error.
    pub fn to_libp2p(peer: PeerId) -> Result<libp2p::PeerId, PeerIdMappingError> {
        let key = ed25519::PublicKey::try_from_bytes(peer.as_bytes())
            .map_err(|_| PeerIdMappingError::MalformedKey)?;

        Ok(PublicKey::from(key).to_peer_id())
    }

    /// The domain [`PeerId`] behind a libp2p identity.
    ///
    /// Fails when the multihash is a *digest* rather than an inlined key
    /// ([`NotInlined`](PeerIdMappingError::NotInlined)) — the key is simply not
    /// recoverable from those bytes — and when the inlined key is not Ed25519
    /// ([`NotEd25519`](PeerIdMappingError::NotEd25519)), which is a peer this
    /// build cannot speak to, since every identity here is an Ed25519 key
    /// (D5).
    pub fn from_libp2p(peer: &libp2p::PeerId) -> Result<PeerId, PeerIdMappingError> {
        let multihash = peer.as_ref();
        if multihash.code() != IDENTITY_HASH_CODE {
            return Err(PeerIdMappingError::NotInlined {
                code: multihash.code(),
            });
        }

        let public = PublicKey::try_decode_protobuf(multihash.digest())
            .map_err(|_| PeerIdMappingError::MalformedKey)?;
        let ed25519 = public
            .try_into_ed25519()
            .map_err(|_| PeerIdMappingError::NotEd25519)?;

        PeerId::from_public_key_bytes(ed25519.to_bytes())
            .map_err(|_| PeerIdMappingError::MalformedKey)
    }
}

/// Why an identity could not be carried across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerIdMappingError {
    /// The libp2p `PeerId` hashes its key instead of inlining it, so the key
    /// is not recoverable from the identity alone. Produced by peers whose
    /// public key is too long to inline — RSA, in practice.
    NotInlined { code: u64 },
    /// The inlined key is a valid libp2p public key of another algorithm.
    NotEd25519,
    /// The bytes are not a well-formed public key encoding at all.
    MalformedKey,
}

impl fmt::Display for PeerIdMappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInlined { code } => write!(
                f,
                "peer identity uses multihash 0x{code:x} rather than an inlined key, \
                 so no public key can be recovered from it"
            ),
            Self::NotEd25519 => f.write_str("peer identity does not carry an Ed25519 public key"),
            Self::MalformedKey => f.write_str("peer identity does not decode to a public key"),
        }
    }
}

impl std::error::Error for PeerIdMappingError {}
