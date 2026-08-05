use shared_types::{PeerConnected, PeerDisconnected};

use crate::domain::{SessionCollapse, SessionDirection};

/// What a [`PeerRoster`](crate::domain::PeerRoster) session transition changed,
/// and what the caller must now do about it.
///
/// The roster cannot close a transport link or publish an event — both are
/// port-shaped, and the domain holds no ports. So a transition *returns* its
/// consequences and the application carries them out. That is also how the
/// cross-context events reach the outside world without this context importing
/// any other: [`connected`](Self::connected) and
/// [`disconnected`](Self::disconnected) are the `shared_types` contracts,
/// handed back for the application to publish.
///
/// Every field is independently `None` in the ordinary case; a plain
/// `Connecting` open changes nothing observable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionOutcome {
    /// A session the caller must now close at the transport, because the
    /// collapse rule discarded it (invariant 3). Named from the local peer's
    /// point of view.
    pub superseded: Option<SessionDirection>,
    /// The collapse decision, when a simultaneous connect was resolved.
    pub collapse: Option<SessionCollapse>,
    /// Publish when present: the peer became reachable.
    pub connected: Option<PeerConnected>,
    /// Publish when present: the peer stopped being reachable.
    pub disconnected: Option<PeerDisconnected>,
}

impl SessionOutcome {
    /// Nothing observable happened.
    pub const fn quiet() -> Self {
        Self {
            superseded: None,
            collapse: None,
            connected: None,
            disconnected: None,
        }
    }
}
