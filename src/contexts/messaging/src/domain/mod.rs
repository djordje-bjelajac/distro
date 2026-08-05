//! Aggregates, value objects, events, and typed errors of the `messaging`
//! context (canvas §2.3).
//!
//! Nothing here depends on `ports` or `adapters`, and nothing here knows how a
//! message travels: there is no `Endpoint`, address, or multiaddress concept
//! anywhere in this crate (canvas §4 — `messaging` addresses peers by `PeerId`
//! alone). Ordering is decided by [`SequenceNumber`] and never by a clock
//! reading; the only instants a message carries are the author's *claim*, kept
//! for display, and — while it waits for a gap to close — the local instant it
//! arrived, which is what bounds that wait (D11, S5, rule R).

pub mod events;

mod author_log;
#[cfg(test)]
mod author_log_test;
mod conversation;
mod conversation_id;
#[cfg(test)]
mod conversation_id_test;
#[cfg(test)]
mod conversation_test;
mod delivery_failure;
mod delivery_state;
#[cfg(test)]
mod delivery_state_test;
mod duration_millis;
#[cfg(test)]
mod duration_millis_test;
mod inbound_outcome;
mod message;
mod message_body;
#[cfg(test)]
mod message_body_test;
mod message_id;
#[cfg(test)]
mod message_id_test;
mod message_placement;
#[cfg(test)]
mod message_test;
mod millis;
#[cfg(test)]
mod millis_test;
mod sequence_number;
#[cfg(test)]
mod sequence_number_test;

pub use author_log::AuthorLog;
pub use conversation::{Conversation, ConversationError};
pub use conversation_id::ConversationId;
pub use delivery_failure::DeliveryFailure;
pub use delivery_state::{DeliveryState, DeliveryStateError};
pub use duration_millis::DurationMillis;
pub use inbound_outcome::InboundOutcome;
pub use message::Message;
pub use message_body::{MessageBody, MessageBodyError};
pub use message_id::MessageId;
pub use message_placement::MessagePlacement;
pub use millis::Millis;
pub use sequence_number::{SequenceNumber, SequenceNumberError};
