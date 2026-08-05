//! The adapters that exist only because three contexts and two infrastructure
//! crates have to be joined, and the assembled [`Node`] that joins them.
//!
//! # What belongs here
//!
//! Everything in this module is *wiring*: a type here either implements a port
//! whose implementation the canvas assigns to "the composition root", or holds
//! a fact no single context owns. None of them decides anything. The
//! distinction is worth keeping sharp, because a root is exactly where a domain
//! rule is easiest to smuggle in and hardest to find later — so each type below
//! states which canvas line put it here:
//!
//! | Type | Why the root owns it |
//! | --- | --- |
//! | [`SystemClock`] | one clock behind both contexts' `ClockPort` (D11, S5) |
//! | [`TrustDirectory`] | `messaging`'s `AuthorPolicyPort` over `identity`'s block list (invariant 11) |
//! | [`MembershipEventRelay`] | `membership`'s events, queued for the one thread that fans them into `messaging` (D10) |
//! | [`MessagingEventSink`] | `messaging`'s events, turned into the markers and counters AC6/AC15 require |
//! | [`CorrelatingTransport`] | the only place a signature and a `MessageId` coexist (AC11) |
//! | [`DeliveryIndex`] | the map that correlation writes and the network's acknowledgement reads |
//! | [`HeartbeatBeacon`] | the liveness probe OP-10 deliberately does not emit |
//! | [`LocalEndpoints`] | where this peer is reachable — no context has an entry for itself |
//! | [`GapLedger`] | abandoned runs, which the read model cannot report (AC15) |
//! | [`Diagnostics`] | the local counters AC6, AC14 and AC15 ask for |
//! | [`NoticeFeed`] | the join account AC3 requires be visible |
//!
//! # What does not belong here
//!
//! Any rule about ordering, trust, delivery, presence, or what a message is.
//! If something the root needs would require such a rule and none exists, that
//! is a canvas gap to report, not a decision to take in a wiring module.

mod correlating_transport;
#[cfg(test)]
mod correlating_transport_test;
mod delivery_index;
#[cfg(test)]
mod delivery_index_test;
mod diagnostics;
mod gap_ledger;
#[cfg(test)]
mod gap_ledger_test;
mod heartbeat_beacon;
#[cfg(test)]
mod heartbeat_beacon_test;
mod local_endpoints;
#[cfg(test)]
mod local_endpoints_test;
mod membership_event_relay;
#[cfg(test)]
mod membership_event_relay_test;
mod messaging_event_sink;
#[cfg(test)]
mod messaging_event_sink_test;
mod node;
#[cfg(test)]
mod node_test;
mod notice_feed;
mod system_clock;
#[cfg(test)]
mod system_clock_test;
mod trust_directory;
#[cfg(test)]
mod trust_directory_test;

pub use correlating_transport::CorrelatingTransport;
pub use delivery_index::DeliveryIndex;
pub use diagnostics::Diagnostics;
pub use gap_ledger::{GapLedger, abandoned_span};
pub use heartbeat_beacon::{HeartbeatBeacon, HeartbeatError};
pub use local_endpoints::LocalEndpoints;
pub use membership_event_relay::MembershipEventRelay;
pub use messaging_event_sink::MessagingEventSink;
pub use node::{Node, NodeSettings, StartError};
pub use notice_feed::{Notice, NoticeFeed, NoticeLevel};
pub use system_clock::SystemClock;
pub use trust_directory::{PeerTrust, TrustDirectory};
