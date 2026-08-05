//! The one thread that keeps an assembled [`Node`](crate::composition::Node)
//! running, and the three pieces of policy it is made of.
//!
//! # What is here, and why it is not in the contexts
//!
//! Every context in this workspace is inert: it starts no task, opens no
//! socket, and reads no clock until something calls it. That is what keeps
//! their tests free of real time (AC13) — and it means somebody has to be the
//! thing that calls them. This module is that somebody.
//!
//! Four of its parts are pure enough to test on their own, and each of them is
//! a place a composition root usually gets something quietly wrong:
//!
//! * [`EventRouter`] — the `NetworkEvent` → inbound-port correspondence, which
//!   `infra-net-libp2p` documents variant by variant and deliberately does not
//!   implement.
//! * [`LifecycleFanout`] — the one seam between two contexts, filtered by the
//!   event's own `is_cross_context`.
//! * [`TickSchedule`] — when the four clock-driven duties are due, two of which
//!   earlier operations flagged as *nothing drives this yet*.
//! * [`delivery_failure_of`] — the transport's failure vocabulary translated
//!   into the domain's, which no crate below the root can do because neither
//!   knows about the other.
//!
//! [`Engine`] is the loop that puts them together. It is deliberately the only
//! part with no unit test of its own: it owns a thread, a network, and a
//! blocking wait, and everything about it that is a decision has been moved
//! into one of the four above.

mod delivery_failure_mapping;
#[cfg(test)]
mod delivery_failure_mapping_test;
mod engine;
mod engine_command;
mod event_router;
#[cfg(test)]
mod event_router_test;
mod lifecycle_fanout;
#[cfg(test)]
mod lifecycle_fanout_test;
mod tick_schedule;
#[cfg(test)]
mod tick_schedule_test;

pub use delivery_failure_mapping::{delivery_failure_of, transport_reason};
pub use engine::{Engine, EngineHandle};
pub use engine_command::EngineCommand;
pub use event_router::EventRouter;
pub use lifecycle_fanout::LifecycleFanout;
pub use tick_schedule::{DueTicks, TickSchedule};
