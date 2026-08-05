//! How every file this crate writes is versioned, parsed, and replaced.
//!
//! # Three rules, and they are S4
//!
//! 1. **Every file begins with `<magic> <version>`.** The magic names the
//!    store, so a file handed to the wrong reader is refused instead of
//!    half-understood; the version is `1` today and moves only when a layout
//!    below it changes.
//! 2. **Reading never writes.** An unknown version yields the port's typed
//!    `UnsupportedSchemaVersion { found }` and leaves the bytes exactly as they
//!    were, so a peer that opens tomorrow's file with today's build can be
//!    downgraded, or upgraded again, with nothing lost. A malformed file yields
//!    the port's `Corrupt` — never a panic, and never a silent reset, because
//!    "the file was odd so I made a new identity" is how a peer loses its name.
//! 3. **A replacement is atomic.** [`private_file::replace_atomically`] writes a
//!    temp file in the same directory, fsyncs it, and renames it over the
//!    target. A crash therefore leaves either the old file or the new one, and
//!    a half-written temp file is never the live file. This matters most to the
//!    sequence counter, where a lost write means a re-issued number and a
//!    silently ignored message (D12).
//!
//! # Why the formats are hand-written
//!
//! No `serde`, no CBOR. The wire codec is confined to OP-10 (D6) and evolves
//! under S2's rule that peers upgrade independently and must tolerate each
//! other; these files are read by exactly one build on one machine and evolve
//! under S4's rule that an unknown version is refused. Different rules, so
//! different mechanisms. A derived encoding would also let an unrelated change
//! to a domain type silently change a file layout; a hand-written parser makes
//! every such change a visible edit here, next to the version number that has
//! to move with it.
//!
//! Each format is line-based UTF-8, documented in full on the store that owns
//! it, and strict: an unparsable line is `Corrupt` rather than something to
//! skip past. Where a field can contain spaces — only endpoint addresses can —
//! it is the last field on its line and runs to the end of it, which needs no
//! escaping because [`Endpoint`](membership::domain::Endpoint) rejects control
//! characters and therefore can never contain a newline.

pub(crate) mod hex_bytes;
#[cfg(test)]
mod hex_bytes_test;
pub(crate) mod private_file;
#[cfg(test)]
mod private_file_test;
mod schema_header;
#[cfg(test)]
mod schema_header_test;
mod versioned_file;
#[cfg(test)]
mod versioned_file_test;

pub(crate) use schema_header::SchemaHeader;
pub(crate) use versioned_file::{LocalFileError, read_versioned, write_versioned};
