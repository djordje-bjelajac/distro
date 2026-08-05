//! The ordered record a scenario asserts on, and the publishers that fill it.
//!
//! One [`EventTrace`] per simulated network, written by every peer's two event
//! publishers and by the fabric itself. That single interleaved stream is what
//! makes "same seed and same script produce a byte-identical trace" a claim
//! about the whole simulation rather than about one layer of it.

mod event_trace;
#[cfg(test)]
mod event_trace_test;
mod membership_event_recorder;
mod messaging_event_recorder;
mod trace_entry;
mod trace_event;

pub use event_trace::EventTrace;
pub use membership_event_recorder::MembershipEventRecorder;
pub use messaging_event_recorder::MessagingEventRecorder;
pub use trace_entry::TraceEntry;
pub use trace_event::{PeerLifecycle, TraceEvent};

pub(crate) use trace_event::label_of;
