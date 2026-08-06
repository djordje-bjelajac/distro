//! Aggregates, value objects, events, and typed errors of the `membership`
//! context (canvas §2.2).
//!
//! Nothing here depends on `ports` or `adapters`, and nothing here knows what a
//! socket is: an [`Endpoint`] is an opaque string plus a reachability class, and
//! every time-dependent rule is a pure function of an explicitly passed
//! [`Millis`] reading (D11) rather than a clock this layer could call.

pub mod events;

mod duration_millis;
#[cfg(test)]
mod duration_millis_test;
mod endpoint;
#[cfg(test)]
mod endpoint_test;
mod join_ticket;
#[cfg(test)]
mod join_ticket_test;
mod known_peer;
mod liveness_windows;
#[cfg(test)]
mod liveness_windows_test;
mod millis;
#[cfg(test)]
mod millis_test;
mod network_status;
#[cfg(test)]
mod network_status_test;
mod peer_roster;
#[cfg(test)]
mod peer_roster_test;
mod peer_standing;
#[cfg(test)]
mod peer_standing_test;
mod presence;
#[cfg(test)]
mod presence_test;
mod reachability;
mod session;
mod session_collapse;
#[cfg(test)]
mod session_collapse_test;
mod session_direction;
mod session_outcome;
mod session_state;
#[cfg(test)]
mod session_test;

pub use duration_millis::DurationMillis;
pub use endpoint::{Endpoint, EndpointError};
pub use join_ticket::{JoinTicket, JoinTicketError};
pub use known_peer::KnownPeer;
pub use liveness_windows::{LivenessWindows, LivenessWindowsError};
pub use millis::Millis;
pub use network_status::NetworkStatus;
pub use peer_roster::{PeerRoster, PeerRosterError};
pub use peer_standing::PeerStanding;
pub use presence::Presence;
pub use reachability::Reachability;
pub use session::{Session, SessionError};
pub use session_collapse::{SessionCollapse, SessionCollapseError};
pub use session_direction::SessionDirection;
pub use session_outcome::SessionOutcome;
pub use session_state::SessionState;
