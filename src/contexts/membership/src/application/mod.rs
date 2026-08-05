//! CQRS handlers and use-case wiring for the `membership` context (canvas §5,
//! OP-6).
//!
//! # Shape
//!
//! Commands live in [`commands`] and queries in [`queries`], separated end to
//! end as `AGENTS.md` requires. The three inbound ports of canvas §4 are
//! implemented by three services —
//! [`JoinNetworkService`](commands::JoinNetworkService) for the deliberate
//! membership decisions, [`InboundSessionService`](commands::InboundSessionService)
//! for everything the network reports, and
//! [`MembershipQueryService`](queries::MembershipQueryService) for the read
//! side — and [`MembershipContext`] assembles all three over one shared
//! [`MembershipState`], so a composition root cannot wire two divergent
//! rosters.
//!
//! # Dependencies
//!
//! `domain` and `ports` only — never an adapter, never another context. Every
//! collaborator arrives as `Arc<dyn …Port + Send + Sync>` through a
//! constructor, so nothing here knows what a socket, a multiaddress grammar,
//! or a file is. That is what lets every test here run against in-memory fakes
//! with a hand-advanced clock and no network (AC13).
//!
//! # The one lock rule
//!
//! No handler holds the roster lock across a call into a port. A port may
//! legitimately call back into this context — a discovery adapter redrawing a
//! status line while the bootstrap ladder runs is the obvious case — and a
//! lock held across the boundary would turn that into a deadlock instead of a
//! read.

pub mod commands;
pub mod queries;

mod join_phase;
mod membership_context;
#[cfg(test)]
mod membership_context_test;
mod membership_settings;
mod membership_state;
#[cfg(test)]
mod membership_state_test;
mod session_outcome_dispatcher;

pub(crate) use join_phase::JoinPhase;
pub use membership_context::MembershipContext;
pub use membership_settings::MembershipSettings;
pub use membership_state::MembershipState;
pub(crate) use session_outcome_dispatcher::SessionOutcomeDispatcher;
