//! `infra-store-fs`: the files a peer keeps between launches — and the one
//! store it deliberately does not keep (OP-11).
//!
//! # What survives a restart, and why each thing does
//!
//! | Store | Survives | Because |
//! | --- | --- | --- |
//! | [`FileIdentityKeyStore`] | yes | AC9: the `PeerId` must be the same one tomorrow, and it is derived from the key |
//! | [`FileTrustRecordStore`] | yes | a fingerprint comparison performed once must not have to be repeated, and a blocked peer must stay blocked |
//! | [`FilePeerCache`] | yes | D1 rung (a): the warm start is what makes a join ticket a one-time cost |
//! | [`FileSequenceCounter`] | yes | D12/AC16: a restarted peer that resumed at 1 was classified a duplicate by everyone still online, and went permanently mute |
//! | [`InMemoryMessageLog`] | **no** | D7: conversation history dies with the process, on purpose |
//! | [`LocalEnvelopeSigner`] | with the key | it *is* the key: rebuilt from the file on every launch, so the peer keeps signing as itself (invariant 4) |
//!
//! The asymmetry in the last two rows is the whole of D12. The counter lives
//! beside the key and shares its lifetime exactly: if the key survives, the
//! counter survives; if the key is gone the identity is gone and starting again
//! at [`SequenceNumber::FIRST`](messaging::domain::SequenceNumber::FIRST) is
//! then correct rather than harmful. `infra-sim-net` models the same split with
//! in-memory stores, and the two crates must agree behaviourally — the
//! simulator is what every multi-peer claim is verified against (S5).
//!
//! # Blocking, synchronous, `std::fs`
//!
//! No `tokio`, no async, no `serde`. These are small local files, read at
//! startup and written when something a human did changes; the cost of a
//! blocking write is a fraction of a millisecond and the cost of an async
//! runtime in an adapter this simple is permanent. Every port here takes
//! `&self` and every store is `Send + Sync`, so a composition root can hold one
//! behind `Arc<dyn …Port + Send + Sync>`.
//!
//! # Signing lives here because the key does
//!
//! [`LocalEnvelopeSigner`] implements **all four** crypto ports — both
//! contexts' `EnvelopeSignerPort` and both contexts' `EnvelopeVerifierPort` —
//! over the one key in the keystore file, which is what canvas §4 means by
//! wiring them all to one underlying implementation. It is reached through
//! [`FileIdentityKeyStore::load_or_create_signer`], the only constructor, so
//! key material never crosses a port boundary to get to a signer: the signer is
//! built where the key already is.
//!
//! # Where to start
//!
//! [`LocalStores`] opens one directory and hands out every store in it, which
//! is what the composition root (OP-12) wants. The individual types stay public
//! for tests, and for a root that wants to place one file somewhere else.

mod crypto;
mod entropy;
#[cfg(test)]
mod entropy_test;
mod format;
mod local_stores;
#[cfg(test)]
mod local_stores_test;
mod stores;
#[cfg(test)]
mod test_dir;
#[cfg(test)]
mod test_peers;

pub use crypto::LocalEnvelopeSigner;
pub use local_stores::{LocalStores, LocalStoresError};
pub use stores::{
    FileIdentityKeyStore, FilePeerCache, FileSequenceCounter, FileTrustRecordStore,
    InMemoryMessageLog,
};
