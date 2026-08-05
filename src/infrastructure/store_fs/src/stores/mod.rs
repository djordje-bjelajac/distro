//! One implementation per store port a real peer runs on.
//!
//! Four of them are files in one directory and one is not; [`crate`] has the
//! table of what survives a restart and why. Each type documents its own
//! on-disk layout in full — that documentation is the format specification,
//! and a change to a layout is a change to the doc comment, the version number
//! in the same file, and the parser, together.

mod file_identity_key_store;
#[cfg(test)]
mod file_identity_key_store_test;
mod file_peer_cache;
#[cfg(test)]
mod file_peer_cache_test;
mod file_sequence_counter;
#[cfg(test)]
mod file_sequence_counter_test;
mod file_trust_record_store;
#[cfg(test)]
mod file_trust_record_store_test;
mod in_memory_message_log;
#[cfg(test)]
mod in_memory_message_log_test;

pub use file_identity_key_store::FileIdentityKeyStore;
pub use file_peer_cache::FilePeerCache;
pub use file_sequence_counter::FileSequenceCounter;
pub use file_trust_record_store::FileTrustRecordStore;
pub use in_memory_message_log::InMemoryMessageLog;
