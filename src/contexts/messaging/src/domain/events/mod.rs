//! Domain events of the `messaging` context (canvas §2.3), all past tense.
//!
//! Every one of them is **context-internal**: this context publishes no
//! cross-context contract, because nothing outside it needs to know that a
//! message exists. Traffic in the other direction — `PeerConnected` and
//! `PeerDisconnected` from `shared_types` — is what this context consumes, and
//! it carries a `PeerId` and nothing else (canvas §4).
//!
//! [`MessagingEvent`] unions them, which is what lets `EventPublisherPort` stay
//! object-safe with a single method.

mod gap_close_cause;
mod message_delivery_state_changed;
mod message_duplicate_ignored;
mod message_gap_closed;
mod message_received;
mod message_rejected;
mod message_sent;
mod messaging_event;
#[cfg(test)]
mod messaging_event_test;
mod rejection_reason;

pub use gap_close_cause::GapCloseCause;
pub use message_delivery_state_changed::MessageDeliveryStateChanged;
pub use message_duplicate_ignored::MessageDuplicateIgnored;
pub use message_gap_closed::MessageGapClosed;
pub use message_received::MessageReceived;
pub use message_rejected::MessageRejected;
pub use message_sent::MessageSent;
pub use messaging_event::MessagingEvent;
pub use rejection_reason::RejectionReason;
