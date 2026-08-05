//! CQRS handlers and use-case wiring for the `identity` context (canvas §5,
//! OP-5).
//!
//! # Shape
//!
//! Commands live in [`commands`] and queries in [`queries`], separated end to
//! end as `AGENTS.md` requires: a command handler mutates and returns only the
//! event its own change produced; a query handler returns a read model and
//! writes nothing, which this crate's query tests assert rather than assume.
//! [`IdentityCommandService`](commands::IdentityCommandService) and
//! [`IdentityQueryService`](queries::IdentityQueryService) implement the
//! inbound ports `IdentityCommandPort` / `IdentityQueryPort`, and
//! [`IdentityContext`] assembles both over one shared [`LocalIdentityState`]
//! so a composition root cannot accidentally wire two divergent views of the
//! local peer.
//!
//! # Dependencies
//!
//! `domain` and `ports` only — never an adapter, never another context. Every
//! collaborator arrives as `Arc<dyn …Port + Send + Sync>` through a
//! constructor, so nothing here knows what a file, a socket, or a key format
//! is. That is also what lets every test here run against in-memory fakes with
//! no network, clock, or filesystem (AC13).

pub mod commands;
pub mod queries;

mod identity_context;
#[cfg(test)]
mod identity_context_test;
mod local_identity_state;
#[cfg(test)]
mod local_identity_state_test;

pub use identity_context::IdentityContext;
pub use local_identity_state::LocalIdentityState;
