//! Deterministic fakes for this context's outbound ports.
//!
//! Test-only (`#[cfg(test)]`) and never linked into a binary. Domain and
//! application tests must touch no network, clock, filesystem, or external
//! service (AC13), so every collaborator these tests need is implemented here
//! in memory.
//!
//! Every fake is `Send + Sync` — it uses atomics and `Mutex` rather than
//! `Cell`/`RefCell` — because the application layer holds its ports as
//! `Arc<dyn …Port + Send + Sync>`, the shape a composition root needs. The
//! locking is uncontended in tests and never a source of nondeterminism.
//!
//! The signature scheme below is **not cryptography** and must never leave
//! tests: it is a deterministic keyed digest that stands in for Ed25519. It
//! gives what the tests actually need — a signature that depends on both the
//! signing peer and the exact bytes signed, so tampering and wrong-signer
//! cases are detectable — without this crate depending on a crypto library.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use shared_types::{Envelope, EnvelopeSignature, PeerId};

use crate::domain::{TrustRecord, UnsignedEnvelope};
use crate::ports::{
    EnvelopeSignerError, EnvelopeSignerPort, EnvelopeVerifierError, EnvelopeVerifierPort,
    IdentityKeyStoreError, IdentityKeyStorePort, SignatureVerdict, TrustRecordStoreError,
    TrustRecordStorePort,
};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Reads a fake's lock without panicking on a poisoned mutex: a fake that
/// failed an assertion in one test must not turn every later test into a
/// panic with a misleading cause.
fn guard<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A deterministic stand-in for `sign(peer_secret_key, message)`.
///
/// Keyed by the signing peer's public bytes, which in a fake is equivalent to
/// keying by its secret: it makes each peer's signatures distinct and lets the
/// matching verifier recompute them from the envelope's author.
pub(crate) fn fake_signature(signer: &PeerId, message: &[u8]) -> EnvelopeSignature {
    let mut bytes = [0u8; EnvelopeSignature::LENGTH];

    for (block, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut hash = FNV_OFFSET_BASIS ^ block as u64;
        for byte in signer.as_bytes().iter().chain(message) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        chunk.copy_from_slice(&hash.to_be_bytes());
    }

    EnvelopeSignature::new(bytes)
}

/// A key store for one peer's keypair, counting how often it was asked for it
/// and how often it had to create it.
///
/// The two constructors are the two launches AC1/AC9 describe:
/// [`empty`](Self::empty) is a fresh install whose first call generates and
/// persists the keypair, [`holding`](Self::holding) is every later launch,
/// which loads it. Callers cannot tell them apart from the returned
/// [`PeerId`] — that is the point — but [`creations`](Self::creations) lets a
/// test assert that a restart created no second key.
pub(crate) struct FakeKeyStore {
    peer: PeerId,
    loads: AtomicUsize,
    creations: AtomicUsize,
    exists: AtomicBool,
}

impl FakeKeyStore {
    /// A store whose keypair already exists: every call loads it.
    pub(crate) const fn holding(peer: PeerId) -> Self {
        Self {
            peer,
            loads: AtomicUsize::new(0),
            creations: AtomicUsize::new(0),
            exists: AtomicBool::new(true),
        }
    }

    /// A store with no key material yet: the first call creates `peer` and
    /// persists it, and every later call loads that same identity.
    pub(crate) const fn empty(peer: PeerId) -> Self {
        Self {
            peer,
            loads: AtomicUsize::new(0),
            creations: AtomicUsize::new(0),
            exists: AtomicBool::new(false),
        }
    }

    pub(crate) fn loads(&self) -> usize {
        self.loads.load(Ordering::Relaxed)
    }

    /// How many keypairs this store had to generate. Never more than one, or
    /// AC9 is broken.
    pub(crate) fn creations(&self) -> usize {
        self.creations.load(Ordering::Relaxed)
    }
}

impl IdentityKeyStorePort for FakeKeyStore {
    fn load_or_create_local_peer(&self) -> Result<PeerId, IdentityKeyStoreError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        if !self.exists.swap(true, Ordering::Relaxed) {
            self.creations.fetch_add(1, Ordering::Relaxed);
        }
        Ok(self.peer)
    }
}

/// A key store that always fails with a given typed error.
pub(crate) struct UnusableKeyStore(pub(crate) IdentityKeyStoreError);

impl IdentityKeyStorePort for UnusableKeyStore {
    fn load_or_create_local_peer(&self) -> Result<PeerId, IdentityKeyStoreError> {
        Err(self.0)
    }
}

/// An in-memory [`TrustRecordStorePort`] that counts reads and writes.
///
/// Records are kept in insertion order, deliberately *not* sorted: a caller
/// that needs a stable order must sort for itself (S5), and a fake that
/// happened to return sorted output would hide a missing sort in the query
/// handler.
pub(crate) struct InMemoryTrustRecordStore {
    records: Mutex<Vec<TrustRecord>>,
    loads: AtomicUsize,
    saves: AtomicUsize,
}

impl InMemoryTrustRecordStore {
    /// A store that has never seen a peer.
    pub(crate) const fn empty() -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            loads: AtomicUsize::new(0),
            saves: AtomicUsize::new(0),
        }
    }

    /// A store already holding `records`, as a previous session would have
    /// left it. Seeding bypasses the counters, so a test's assertions describe
    /// only the calls the code under test made.
    pub(crate) fn seeded_with(records: impl IntoIterator<Item = TrustRecord>) -> Self {
        let store = Self::empty();
        guard(&store.records).extend(records);
        store
    }

    pub(crate) fn loads(&self) -> usize {
        self.loads.load(Ordering::Relaxed)
    }

    pub(crate) fn saves(&self) -> usize {
        self.saves.load(Ordering::Relaxed)
    }

    /// Reads a record without going through the port, so inspecting the store
    /// in an assertion does not disturb the counters it is asserting on.
    pub(crate) fn stored(&self, peer: PeerId) -> Option<TrustRecord> {
        guard(&self.records)
            .iter()
            .find(|record| record.peer() == peer)
            .cloned()
    }
}

impl TrustRecordStorePort for InMemoryTrustRecordStore {
    fn load_trust_record(
        &self,
        peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        Ok(self.stored(peer))
    }

    fn save_trust_record(&self, record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        self.saves.fetch_add(1, Ordering::Relaxed);

        let mut records = guard(&self.records);
        match records
            .iter_mut()
            .find(|stored| stored.peer() == record.peer())
        {
            Some(stored) => *stored = record.clone(),
            None => records.push(record.clone()),
        }
        Ok(())
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        Ok(guard(&self.records)
            .iter()
            .filter(|record| record.is_blocked())
            .map(TrustRecord::peer)
            .collect())
    }
}

/// A trust record store that always fails with a given typed error.
pub(crate) struct UnusableTrustRecordStore(pub(crate) TrustRecordStoreError);

impl TrustRecordStorePort for UnusableTrustRecordStore {
    fn load_trust_record(
        &self,
        _peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        Err(self.0)
    }

    fn save_trust_record(&self, _record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        Err(self.0)
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        Err(self.0)
    }
}

/// A trust record store that reads as empty but cannot write.
///
/// Separates the half-applied case — the domain accepted the transition, the
/// store then refused it — from a store that is unusable end to end.
pub(crate) struct UnwritableTrustRecordStore(pub(crate) TrustRecordStoreError);

impl TrustRecordStorePort for UnwritableTrustRecordStore {
    fn load_trust_record(
        &self,
        _peer: PeerId,
    ) -> Result<Option<TrustRecord>, TrustRecordStoreError> {
        Ok(None)
    }

    fn save_trust_record(&self, _record: &TrustRecord) -> Result<(), TrustRecordStoreError> {
        Err(self.0)
    }

    fn list_blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError> {
        Ok(Vec::new())
    }
}

/// A signer holding one peer's key that records the exact bytes it was asked
/// to sign.
pub(crate) struct RecordingSigner {
    peer: PeerId,
    signed_inputs: Mutex<Vec<Vec<u8>>>,
}

impl RecordingSigner {
    pub(crate) const fn holding_key_of(peer: PeerId) -> Self {
        Self {
            peer,
            signed_inputs: Mutex::new(Vec::new()),
        }
    }

    /// Every byte string handed to [`EnvelopeSignerPort::sign`], in order.
    pub(crate) fn signed_inputs(&self) -> Vec<Vec<u8>> {
        guard(&self.signed_inputs).clone()
    }
}

impl EnvelopeSignerPort for RecordingSigner {
    fn sign(&self, unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        let message = unsigned.signable_bytes();
        let signature = fake_signature(&self.peer, &message);
        guard(&self.signed_inputs).push(message);
        Ok(signature)
    }
}

/// A signer that always fails with a given typed error.
pub(crate) struct FailingSigner(pub(crate) EnvelopeSignerError);

impl EnvelopeSignerPort for FailingSigner {
    fn sign(&self, _unsigned: &UnsignedEnvelope) -> Result<EnvelopeSignature, EnvelopeSignerError> {
        Err(self.0)
    }
}

/// The verifier matching [`RecordingSigner`]: recomputes the expected
/// signature from the envelope's own author and signable bytes.
pub(crate) struct CheckingVerifier;

impl EnvelopeVerifierPort for CheckingVerifier {
    fn verify(&self, envelope: &Envelope) -> Result<SignatureVerdict, EnvelopeVerifierError> {
        let expected = fake_signature(&envelope.author, &envelope.signable_bytes());

        Ok(if expected == envelope.signature {
            SignatureVerdict::Valid
        } else {
            SignatureVerdict::Invalid
        })
    }
}

/// A verifier that cannot perform the check at all.
pub(crate) struct UnavailableVerifier;

impl EnvelopeVerifierPort for UnavailableVerifier {
    fn verify(&self, _envelope: &Envelope) -> Result<SignatureVerdict, EnvelopeVerifierError> {
        Err(EnvelopeVerifierError::VerifierUnavailable)
    }
}
