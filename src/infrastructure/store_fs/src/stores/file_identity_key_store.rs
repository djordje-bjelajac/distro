use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ed25519_dalek::SigningKey;
use identity::ports::{IdentityKeyStoreError, IdentityKeyStorePort};
use shared_types::PeerId;

use crate::crypto::{LocalEnvelopeSigner, peer_of};
use crate::entropy;
use crate::format::hex_bytes::KEY_BYTES;
use crate::format::{LocalFileError, SchemaHeader, hex_bytes, private_file, read_versioned};

/// The local peer's Ed25519 keypair, in one owner-only file (D5, AC1, AC9).
///
/// # The file
///
/// ```text
/// distro-identity-key 1
/// ed25519-seed <64 lowercase hex characters>
/// ```
///
/// Two lines, nothing else; a third line is corruption. The header is S4's
/// version discipline (see [`crate::format`]).
///
/// **Only the secret seed is stored, never the public key.** The public half is
/// derived on every load, which makes an identity–key mismatch unrepresentable
/// on disk (invariant 1) — there is no second field that could disagree with
/// the first, so no file can claim a `PeerId` its key does not produce. Any 32
/// bytes are a valid seed, so the derivation cannot fail once the hex parses.
///
/// # Load-or-create, with nobody asked anything
///
/// First launch generates a seed from the OS random source, writes it, and
/// returns the derived `PeerId`; every later call reads that file and returns
/// the same `PeerId` (AC1, AC9). No prompt, no configuration, no registration
/// step exists to skip.
///
/// The create half uses an exclusive create rather than the atomic rename the
/// other stores use, because a rename would clobber: two processes starting on
/// one profile directory at the same moment would each publish a keypair, and
/// the loser would silently change identity. Here the loser gets
/// `AlreadyExists`, re-reads, and adopts the identity that won. See
/// [`crate::format::private_file`] for the full trade.
///
/// # Secret bytes stop here
///
/// The port returns a [`PeerId`] and this type exposes nothing else. The seed
/// is read into a local buffer, turned into a [`SigningKey`], and dropped —
/// `SigningKey` zeroizes on drop — and no accessor, `Debug`, or `Display` on
/// this type can reach it.
///
/// # The signer comes from here, and only from here
///
/// Signing is a separate port, but its implementation needs the one thing this
/// file holds, so [`load_or_create_signer`](Self::load_or_create_signer) builds
/// it: a [`LocalEnvelopeSigner`] that owns the [`SigningKey`] and hands out
/// signatures. That constructor is crate-private, so a signer can only ever
/// come from a key file — there is no way to assemble one over key material
/// that arrived some other way.
///
/// Both entry points share one load-or-create path, so the port's idempotence
/// and the signer's identity cannot drift: whichever you call first creates the
/// file, and every later call of either kind reads that same key.
///
/// **Known limitation, stated rather than assumed away:** the seed passes
/// through ordinary buffers on the way to and from the file — a `[u8; 32]` and
/// a hex `String` when creating, the `Vec<u8>` of the file read when loading —
/// and none of those is wiped when it is dropped. A determined local attacker
/// who can read this process's memory or a swap page can therefore recover the
/// key, and so can one who can simply read the file, which is the same
/// exposure. Closing the memory half means holding every one of those buffers
/// in `Zeroizing` (the `zeroize` crate is already in the tree beneath
/// `ed25519-dalek`, but is not a direct dependency of this crate); zeroing an
/// array by hand is not that, since the compiler may elide the write.
pub struct FileIdentityKeyStore {
    path: PathBuf,
    /// Serialises load-or-create within one process, so two threads racing on
    /// first launch cannot both generate a keypair and disagree about which one
    /// is this peer.
    gate: Mutex<()>,
}

impl FileIdentityKeyStore {
    /// The conventional file name inside a store directory.
    pub const FILE_NAME: &'static str = "identity.key";

    /// The header every version of this file carries.
    const HEADER: SchemaHeader = SchemaHeader::new("distro-identity-key", 1);

    /// The tag naming what the second line holds. Present so a future v2 that
    /// adds a second algorithm can be told apart line by line rather than by
    /// position.
    const SEED_TAG: &'static str = "ed25519-seed";

    /// A store keeping its key at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            gate: Mutex::new(()),
        }
    }

    /// Where this store keeps its key.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The signer speaking for the identity in this file, creating that
    /// identity on first use exactly as
    /// [`load_or_create_local_peer`](IdentityKeyStorePort::load_or_create_local_peer)
    /// does.
    ///
    /// This is what the composition root calls to wire both contexts' signer
    /// and verifier ports (canvas §4). The returned value owns the key; this
    /// store keeps no copy, and nothing else in the crate can build one.
    pub fn load_or_create_signer(&self) -> Result<LocalEnvelopeSigner, IdentityKeyStoreError> {
        Ok(LocalEnvelopeSigner::new(self.load_or_create_signing_key()?))
    }

    /// Writes this identity's raw Ed25519 secret into `destination`, for the
    /// **transport handshake and nothing else** (canvas S3a).
    ///
    /// # Why this method exists, given everything above
    ///
    /// The libp2p Noise and TLS handshakes prove that the peer on the other end
    /// holds the private key behind its `PeerId`. That proof is what makes a
    /// session *authenticated* rather than merely encrypted, and it happens
    /// inside the swarm on bytes the swarm owns — so unlike signing, it cannot
    /// be delegated behind a port. Safeguard S3a records the consequence: the
    /// **port** still never exposes secret bytes and `IdentityKeyStorePort`
    /// still returns only a `PeerId`; the transport identity is obtained by the
    /// composition root through this narrow, explicitly-named method on the
    /// **concrete** store, and passed straight into
    /// `NetworkIdentity::from_ed25519_secret_key`.
    ///
    /// Two consumers of the secret exist by design — the signer and the
    /// transport handshake — and both live in infrastructure beside the key.
    /// This is the second one, and there is no third.
    ///
    /// # The contract on the caller
    ///
    /// * Pass the buffer directly to `NetworkIdentity::from_ed25519_secret_key`,
    ///   which **zeroes it** as it consumes it (`libp2p` clears the slice it is
    ///   given). Nothing copies it first, so one fewer copy of a secret exists
    ///   in memory.
    /// * If that call is not reached — an early return, an error path — zero
    ///   the buffer with [`zeroize`](Self::zeroize) before dropping it.
    /// * Never log it, never return it, never store it anywhere else. The
    ///   `&mut [u8; 32]` out-parameter rather than a return value is deliberate:
    ///   a returned array is trivially bound to a `let` that outlives its use
    ///   and printed with `{:?}`, whereas a buffer the caller already owns is
    ///   one it is already responsible for.
    ///
    /// The same known limitation stated on this type applies: the seed passes
    /// through the file-read buffer on its way here, and that buffer is not
    /// wiped.
    ///
    /// Load-or-create, like every other entry point: on first launch this
    /// creates the identity rather than failing, so a fresh install reaches a
    /// listening node with nobody asked anything (AC1).
    pub fn load_or_create_transport_secret_key(
        &self,
        destination: &mut [u8; KEY_BYTES],
    ) -> Result<PeerId, IdentityKeyStoreError> {
        let key = self.load_or_create_signing_key()?;

        destination.copy_from_slice(key.as_bytes());

        // `key` drops here and `SigningKey` zeroizes on drop, so the only copy
        // left is the caller's buffer, which the caller has just been told what
        // to do with.
        Ok(peer_of(&key))
    }

    /// Overwrites a secret buffer.
    ///
    /// For the caller's error paths — the success path hands the buffer to
    /// `NetworkIdentity::from_ed25519_secret_key`, which clears it. A plain
    /// loop with a volatile write per byte, rather than `fill(0)`, because the
    /// compiler is entitled to elide a write to memory nothing reads again, and
    /// that elision is exactly what would leave the key in place.
    pub fn zeroize(buffer: &mut [u8; KEY_BYTES]) {
        for byte in buffer.iter_mut() {
            // SAFETY: `byte` is a valid, aligned, initialised `u8` this
            // function holds a unique reference to.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }

    /// The one load-or-create path, shared by the port and the signer.
    ///
    /// Both public entry points come through here, so they cannot drift: the
    /// `PeerId` the port reports is by construction the public half of the key
    /// the signer signs with.
    fn load_or_create_signing_key(&self) -> Result<SigningKey, IdentityKeyStoreError> {
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match self.load()? {
            Some(key) => Ok(key),
            None => self.create(),
        }
    }

    fn load(&self) -> Result<Option<SigningKey>, IdentityKeyStoreError> {
        let Some(body) = read_versioned(&self.path, &Self::HEADER).map_err(to_port_error)? else {
            return Ok(None);
        };

        let [line] = body.as_slice() else {
            return Err(IdentityKeyStoreError::Corrupt);
        };

        let seed = line
            .strip_prefix(Self::SEED_TAG)
            .and_then(|rest| rest.strip_prefix(' '))
            .and_then(hex_bytes::decode)
            .ok_or(IdentityKeyStoreError::Corrupt)?;

        Ok(Some(SigningKey::from_bytes(&seed)))
    }

    fn create(&self) -> Result<SigningKey, IdentityKeyStoreError> {
        let mut seed = [0u8; KEY_BYTES];
        entropy::fill_secret(&mut seed).map_err(|_| IdentityKeyStoreError::CreationFailed)?;

        let text = format!(
            "{}\n{} {}\n",
            Self::HEADER.line(),
            Self::SEED_TAG,
            hex_bytes::encode(&seed)
        );

        match private_file::create_exclusively(&self.path, text.as_bytes()) {
            Ok(()) => Ok(SigningKey::from_bytes(&seed)),
            // Another process created the file between our read and our write.
            // Its identity is the one that exists on disk, so adopt it rather
            // than insisting on the key we happened to generate: AC9 is about
            // the file, not about this call.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.load()?.ok_or(IdentityKeyStoreError::CreationFailed)
            }
            Err(_) => Err(IdentityKeyStoreError::CreationFailed),
        }
    }
}

impl IdentityKeyStorePort for FileIdentityKeyStore {
    fn load_or_create_local_peer(&self) -> Result<PeerId, IdentityKeyStoreError> {
        // The key is derived, read, and dropped here: a caller asking who this
        // peer is never holds anything secret, and `SigningKey` zeroizes on
        // drop.
        self.load_or_create_signing_key().map(|key| peer_of(&key))
    }
}

impl std::fmt::Debug for FileIdentityKeyStore {
    /// Hand-written so no future field can print key material by accident. The
    /// path is the whole of what is safe to say about a keystore.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileIdentityKeyStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Translates a file failure into the port's vocabulary.
///
/// `WriteFailed` cannot reach here — reading never writes — but the match is
/// exhaustive so that a future variant has to be considered rather than
/// defaulted.
const fn to_port_error(error: LocalFileError) -> IdentityKeyStoreError {
    match error {
        LocalFileError::Unreadable => IdentityKeyStoreError::Unreadable,
        LocalFileError::Corrupt => IdentityKeyStoreError::Corrupt,
        LocalFileError::UnsupportedSchemaVersion { found } => {
            IdentityKeyStoreError::UnsupportedSchemaVersion { found }
        }
        LocalFileError::WriteFailed => IdentityKeyStoreError::CreationFailed,
    }
}
