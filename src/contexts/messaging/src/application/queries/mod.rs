//! Query handlers: the paths that only read `messaging` state.
//!
//! No handler here writes — not to a conversation, not to the log, not to the
//! counter. None of them can reach a mutating entry point at all: they hold the
//! registry only through `ConversationRegistry::read`, which never opens a
//! conversation, so rendering an empty pane cannot bring a conversation into
//! existence. This crate's query tests assert that rather than trust it.
//!
//! # What a read can never show
//!
//! A buffered arrival. It is not part of the conversation yet (invariant 5),
//! and showing it would put an author's messages out of that author's send
//! order — the one thing the sequencing rules exist to prevent (AC8). It is a
//! property of what these handlers read, not a filter someone could forget:
//! the aggregate does not expose held messages at all.
//!
//! Content that failed signature verification is equally unreachable — it never
//! reaches a conversation (invariant 10, AC6).

mod get_conversation_history;
#[cfg(test)]
mod get_conversation_history_test;
mod get_message_delivery_state;
#[cfg(test)]
mod get_message_delivery_state_test;
mod list_conversations;
#[cfg(test)]
mod list_conversations_test;
mod messaging_query_service;

pub use get_conversation_history::{GetConversationHistory, GetConversationHistoryHandler};
pub use get_message_delivery_state::{GetMessageDeliveryState, GetMessageDeliveryStateHandler};
pub use list_conversations::{ListConversations, ListConversationsHandler};
pub use messaging_query_service::MessagingQueryService;
