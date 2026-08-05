use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::format::private_file;
use crate::stores::{
    FileIdentityKeyStore, FilePeerCache, FileSequenceCounter, FileTrustRecordStore,
    InMemoryMessageLog,
};

/// Every store a peer runs on, opened in one directory.
///
/// # What the composition root gets
///
/// One call — [`LocalStores::open`] — creates the directory if it is not there
/// and hands back all five stores, each behind an [`Arc`] so it can be coerced
/// to `Arc<dyn …Port + Send + Sync>` and shared with the contexts that need it.
/// The point is that the keypair, the trust records, the peer cache and the
/// sequence counter end up in the *same* directory without the root having to
/// know that D12 requires it: the counter shares the keypair's lifetime because
/// it shares its directory, and deleting that directory discards a whole
/// identity coherently.
///
/// The message log is handed out from here too even though it is in memory
/// (D7), so a root has one place to get every store and no reason to build one
/// of them differently.
///
/// # Getting the signer
///
/// The four crypto ports are **not** accessors on this type, because obtaining
/// a signer reads (or creates) the key file and can fail, while everything
/// here is infallible and does no I/O. The root asks the keystore instead:
///
/// ```no_run
/// # use std::sync::Arc;
/// # use infra_store_fs::{LocalStores, LocalEnvelopeSigner};
/// # fn wire(profile_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
/// let stores = LocalStores::open(profile_dir)?;
/// let signer = Arc::new(stores.identity_keys().load_or_create_signer()?);
///
/// // All four ports, one object, one key (canvas §4). Note `signer.clone()`
/// // rather than `Arc::clone(&signer)`: the unsizing coercion to a trait
/// // object happens at the annotated binding, and the turbofish-free form is
/// // the one that compiles.
/// let identity_signer: Arc<dyn identity::ports::EnvelopeSignerPort + Send + Sync> =
///     signer.clone();
/// let identity_verifier: Arc<dyn identity::ports::EnvelopeVerifierPort + Send + Sync> =
///     signer.clone();
/// let messaging_signer: Arc<dyn messaging::ports::EnvelopeSignerPort + Send + Sync> =
///     signer.clone();
/// let messaging_verifier: Arc<dyn messaging::ports::EnvelopeVerifierPort + Send + Sync> =
///     signer;
/// # let _ = (identity_signer, identity_verifier, messaging_signer, messaging_verifier);
/// # Ok(())
/// # }
/// ```
///
/// That call and [`identity_keys`](Self::identity_keys)`.load_or_create_local_peer()`
/// share one load-or-create path, so calling either first is safe and both
/// report the same identity ([`LocalEnvelopeSigner`](crate::LocalEnvelopeSigner)).
///
/// # Layout
///
/// ```text
/// <root>/                    0700 on unix
/// ├── identity.key           0600 — the Ed25519 secret seed (D5)
/// ├── trust.records          0600 — verification and block state
/// ├── peers.cache            0600 — the warm-start peer set (D1)
/// └── sequence.counter       0600 — outbound marks per conversation (D12)
/// ```
///
/// Choosing `<root>` is the root's business, not this crate's: a platform
/// config-directory lookup belongs where the binary's identity is known, and
/// hard-coding one here would make every test and every second instance fight
/// over it.
#[derive(Debug, Clone)]
pub struct LocalStores {
    root: PathBuf,
    identity_keys: Arc<FileIdentityKeyStore>,
    trust_records: Arc<FileTrustRecordStore>,
    peer_cache: Arc<FilePeerCache>,
    sequence_counter: Arc<FileSequenceCounter>,
    message_log: Arc<InMemoryMessageLog>,
}

impl LocalStores {
    /// Opens — creating if needed — the store directory at `root`.
    ///
    /// Nothing is read here and no file is created: the stores are lazy, so a
    /// directory that has never been used is indistinguishable from one whose
    /// files were deleted, which is what makes first launch and a reset behave
    /// identically (AC1).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LocalStoresError> {
        let root = root.into();

        private_file::create_owner_only_directory(&root)
            .map_err(|_| LocalStoresError::DirectoryUnavailable)?;

        Ok(Self {
            identity_keys: Arc::new(FileIdentityKeyStore::at(
                root.join(FileIdentityKeyStore::FILE_NAME),
            )),
            trust_records: Arc::new(FileTrustRecordStore::at(
                root.join(FileTrustRecordStore::FILE_NAME),
            )),
            peer_cache: Arc::new(FilePeerCache::at(root.join(FilePeerCache::FILE_NAME))),
            sequence_counter: Arc::new(FileSequenceCounter::at(
                root.join(FileSequenceCounter::FILE_NAME),
            )),
            message_log: Arc::new(InMemoryMessageLog::default()),
            root,
        })
    }

    /// The directory these stores live in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `identity`'s `IdentityKeyStorePort` (D5, AC9) — and the source of the
    /// signer, via
    /// [`load_or_create_signer`](FileIdentityKeyStore::load_or_create_signer);
    /// see the type docs.
    pub fn identity_keys(&self) -> Arc<FileIdentityKeyStore> {
        Arc::clone(&self.identity_keys)
    }

    /// `identity`'s `TrustRecordStorePort` — also the block list the root
    /// adapts into `messaging`'s `AuthorPolicyPort` (invariant 11).
    pub fn trust_records(&self) -> Arc<FileTrustRecordStore> {
        Arc::clone(&self.trust_records)
    }

    /// `membership`'s `PeerCachePort` (D1, rung (a)).
    pub fn peer_cache(&self) -> Arc<FilePeerCache> {
        Arc::clone(&self.peer_cache)
    }

    /// `messaging`'s `SequenceCounterPort` (D12, AC16).
    pub fn sequence_counter(&self) -> Arc<FileSequenceCounter> {
        Arc::clone(&self.sequence_counter)
    }

    /// `messaging`'s `MessageLogPort` — in memory, and it dies with the process
    /// (D7).
    pub fn message_log(&self) -> Arc<InMemoryMessageLog> {
        Arc::clone(&self.message_log)
    }
}

/// Typed failure of opening a store directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalStoresError {
    /// The directory could not be created or made owner-only. Nothing was
    /// written and no identity was created; a caller must report this rather
    /// than continue with stores that cannot persist anything.
    DirectoryUnavailable,
}

impl fmt::Display for LocalStoresError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryUnavailable => f.write_str("the local store directory is unavailable"),
        }
    }
}

impl std::error::Error for LocalStoresError {}
