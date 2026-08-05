//! `app`: the composition root, the terminal interface, and the binary
//! (canvas D8, D9, OP-12).
//!
//! # What this crate is
//!
//! The one place every other crate meets. It depends on all three bounded
//! contexts and both production infrastructure crates, and **nothing depends on
//! it** (canvas §4) — which is what makes it the only crate that may know how
//! the whole thing is put together.
//!
//! It does not, however, get to decide anything. Every rule about ordering,
//! trust, presence, delivery, or what a message is belongs to a context. The
//! root's job is to hand each context the collaborators its ports name, to
//! drive the two clock ticks nothing else drives, and to render. Where a wiring
//! decision has a real consequence — the clock's origin, the block-list cache,
//! the delivery correlation — the type that makes it says so and says why.
//!
//! The one thing the root does translate is vocabulary: the network's failure
//! reasons into the domain's, in `runtime::delivery_failure_of`, because
//! neither crate below the root knows the other exists. Where that translation
//! cannot be exact it says so at the mapping site and keeps the transport's own
//! sentence in the notice beside the message, rather than coarsening what the
//! user is told to fit what the domain records.
//!
//! # `infra-sim-net` is not a dependency, on purpose
//!
//! The simulator has a virtual clock and an in-process fabric. A production
//! binary that could link it could ship a build whose "network" was a
//! `HashMap`, so it is absent from `Cargo.toml` and its absence is part of the
//! design (canvas OP-8: "Never linked into the `app` binary").
//!
//! # The four modules
//!
//! | Module | What it holds |
//! | --- | --- |
//! | [`cli`] | arguments, the profile directory, and the usage text carrying S7/S8 |
//! | [`composition`] | the root-owned adapters, and the assembled `Node` |
//! | [`runtime`] | the one thread that drains, fans out, and ticks |
//! | [`tui`] | the terminal interface (D8) — thin, with its view models tested apart from the screen |
//!
//! # Where to start
//!
//! [`composition::Node::start`] documents the startup order and why it is that
//! order; [`runtime::Engine`] documents the threading; [`tui::run`] is the
//! interface's loop.

pub mod cli;
pub mod composition;
pub mod runtime;
pub mod tui;

#[cfg(test)]
mod required_network;
#[cfg(test)]
mod required_network_test;
#[cfg(test)]
mod test_dir;
#[cfg(test)]
mod test_peers;
