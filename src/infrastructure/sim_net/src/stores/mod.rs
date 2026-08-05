//! In-memory implementations of every store port a simulated peer runs on.
//!
//! # Two lifetimes, and the difference is load-bearing
//!
//! The harness splits these into what survives a simulated restart and what
//! does not, because that split *is* D12 and AC16:
//!
//! * **Survives** — [`InMemoryPeerCache`] (D1's warm start),
//!   [`InMemoryTrustRecords`] (a verification performed once, a block that must
//!   stay), and [`PersistentSequenceCounter`] (the keypair's lifetime, exactly).
//!   The harness holds these beside the keypair and hands the same instances to
//!   every rebuild.
//! * **Does not** — [`InMemoryMessageLog`] (D7: conversation history dies with
//!   the process). A fresh one is built on every rebuild.
//!
//! A restarted peer therefore continues its outbound sequence into
//! conversations it no longer remembers — the exact condition that used to make
//! it permanently mute, and the one AC16 pins.
//!
//! # Interior mutability, no threads
//!
//! Every port takes `&self` and is held behind `Arc<dyn …Port + Send + Sync>`,
//! so these use `Mutex` rather than `RefCell`. Nothing here spawns a thread and
//! nothing blocks: the locks are uncontended and are never a source of
//! nondeterminism.

mod in_memory_message_log;
#[cfg(test)]
mod in_memory_message_log_test;
mod in_memory_peer_cache;
#[cfg(test)]
mod in_memory_peer_cache_test;
mod in_memory_trust_records;
#[cfg(test)]
mod in_memory_trust_records_test;
mod persistent_sequence_counter;
#[cfg(test)]
mod persistent_sequence_counter_test;
mod trust_record_author_policy;

pub use in_memory_message_log::InMemoryMessageLog;
pub use in_memory_peer_cache::InMemoryPeerCache;
pub use in_memory_trust_records::InMemoryTrustRecords;
pub use persistent_sequence_counter::PersistentSequenceCounter;
pub use trust_record_author_policy::TrustRecordAuthorPolicy;

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Reads a lock without panicking on a poisoned mutex.
///
/// A scenario that failed an assertion while holding one must not turn every
/// later test into a panic with a misleading cause: the first failure is the
/// one worth reading.
pub(crate) fn guard<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}
