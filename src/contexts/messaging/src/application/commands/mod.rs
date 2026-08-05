//! Command handlers: the paths that change `messaging` state.
//!
//! Each command is an imperative DTO naming a use case, each handler is named
//! by intent, and every handler returns the past-tense event its change
//! produced — or a typed outcome that says what happened instead. Nothing here
//! returns a read model; that is [`queries`](crate::application::queries).
//!
//! The commands live here rather than in `ports/` because a port may depend on
//! `domain` and `shared_types` only. The inbound ports therefore speak in
//! domain types, and the three services below build these DTOs from them.
//!
//! # The two send paths never merge
//!
//! [`SendDirectMessage`] and [`PublishBroadcastMessage`] are separate commands
//! with separate handlers, and their two steps run in opposite orders. That is
//! not duplication: a direct message must exist locally *before* it is sent so
//! a refusal is visible as `Failed(reason)` (AC11), and a broadcast must be
//! published *before* it is recorded, because it has no failed state and must
//! never claim a publication that did not happen (D3, AC10).

mod accept_inbound_message;
#[cfg(test)]
mod accept_inbound_message_test;
mod close_aged_gaps;
#[cfg(test)]
mod close_aged_gaps_test;
mod fail_pending_directs;
mod inbound_envelope_service;
mod mark_message_delivered;
#[cfg(test)]
mod mark_message_delivered_test;
mod open_direct_conversation;
mod outbound_composer;
mod peer_lifecycle_service;
#[cfg(test)]
mod peer_lifecycle_service_test;
mod publish_broadcast_message;
#[cfg(test)]
mod publish_broadcast_message_test;
mod send_direct_message;
#[cfg(test)]
mod send_direct_message_test;
mod send_message_service;

pub use accept_inbound_message::{AcceptInboundMessage, AcceptInboundMessageHandler};
pub use close_aged_gaps::{CloseAgedGaps, CloseAgedGapsHandler};
pub use fail_pending_directs::{FailPendingDirects, FailPendingDirectsHandler};
pub use inbound_envelope_service::InboundEnvelopeService;
pub use mark_message_delivered::{MarkMessageDelivered, MarkMessageDeliveredHandler};
pub use open_direct_conversation::{OpenDirectConversation, OpenDirectConversationHandler};
pub use peer_lifecycle_service::PeerLifecycleService;
pub use publish_broadcast_message::{PublishBroadcastMessage, PublishBroadcastMessageHandler};
pub use send_direct_message::{SendDirectMessage, SendDirectMessageHandler};
pub use send_message_service::SendMessageService;

pub(crate) use outbound_composer::OutboundComposer;
