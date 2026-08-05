//! CQRS handlers and use-case wiring for the `messaging` context (canvas §5,
//! OP-7).
//!
//! # Shape
//!
//! Commands live in [`commands`] and queries in [`queries`], separated end to
//! end as `AGENTS.md` requires. The four inbound ports of canvas §4 are
//! implemented by four services —
//! [`SendMessageService`](commands::SendMessageService) for composing,
//! [`InboundEnvelopeService`](commands::InboundEnvelopeService) for everything
//! the network reports about messages,
//! [`PeerLifecycleService`](commands::PeerLifecycleService) for the peer
//! connect/disconnect news `membership` publishes (D10), and
//! [`MessagingQueryService`](queries::MessagingQueryService) for reads — and
//! [`MessagingContext`] assembles all four over one shared
//! [`ConversationRegistry`], so a composition root cannot wire two divergent
//! views of the same conversation.
//!
//! # Dependencies
//!
//! `domain` and `ports` only — never an adapter, never another context. Every
//! collaborator arrives as `Arc<dyn …Port + Send + Sync>` through a
//! constructor, so nothing here knows what a socket, a codec, or a key format
//! is. That is what lets every test here run against in-memory fakes with a
//! hand-advanced clock and no network (AC13).
//!
//! There is no address anywhere in this layer: peers are named by `PeerId`
//! alone, and how one is reached belongs entirely to `membership` (canvas §4).
//!
//! # The one lock rule
//!
//! No handler holds the registry lock across a call into a port. A port may
//! legitimately call back into this context — a transport adapter asking what a
//! conversation holds while a send is in flight — and a lock held across the
//! boundary would turn that into a deadlock instead of a read. Every handler
//! therefore runs pure domain work under the lock, takes the events back out,
//! and publishes them after the lock is released.
//!
//! # Two instants, and only one of them is a fact
//!
//! Every instant this layer *uses* comes from
//! [`ClockPort`](crate::ports::ClockPort) (D11, S5). The author's claimed send
//! time arrives on the wire and is carried for display only: it never ages a
//! gap, never orders anything, and never decides a duplicate. Anything else
//! would let a peer change this peer's behaviour by lying about the time.

pub mod commands;
pub mod queries;

mod conversation_registry;
#[cfg(test)]
mod conversation_registry_test;
mod message_recorder;
mod messaging_context;
#[cfg(test)]
mod messaging_context_test;
mod messaging_ports;
mod messaging_settings;
#[cfg(test)]
pub(crate) mod test_context;

pub use conversation_registry::ConversationRegistry;
pub use messaging_context::MessagingContext;
pub use messaging_ports::MessagingPorts;
pub use messaging_settings::MessagingSettings;

pub(crate) use message_recorder::MessageRecorder;
