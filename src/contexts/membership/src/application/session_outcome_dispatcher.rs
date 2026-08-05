use std::sync::Arc;

use crate::domain::SessionOutcome;
use crate::ports::{EventPublisherError, EventPublisherPort};

/// Publishes the cross-context events a session transition produced.
///
/// The roster cannot publish — it holds no ports — so every transition hands
/// back a [`SessionOutcome`] and this carries out the announcing half of it.
/// Sharing one implementation across the four handlers that change sessions is
/// what keeps `PeerConnected`/`PeerDisconnected` from being emitted in four
/// slightly different ways.
///
/// # What it deliberately does not do
///
/// It does not close the superseded link of a simultaneous connect.
/// `PeerTransportPort::close_session` closes *by peer*, and during a collapse
/// this peer holds two links to that same peer — closing by peer would take
/// the survivor with it. Only the adapter that accepted the two links can tell
/// them apart, so [`SessionOutcome::superseded`] is handed back to the caller
/// and the close happens where the link handles live.
#[derive(Clone)]
pub(crate) struct SessionOutcomeDispatcher {
    publisher: Arc<dyn EventPublisherPort + Send + Sync>,
}

impl SessionOutcomeDispatcher {
    pub(crate) fn new(publisher: Arc<dyn EventPublisherPort + Send + Sync>) -> Self {
        Self { publisher }
    }

    /// Publishes whatever the outcome carries, disconnects before connects.
    ///
    /// The order matters across a collapse: a consumer that saw the connect
    /// first and the disconnect second would end up believing a link exists
    /// while its bytes go nowhere.
    pub(crate) fn publish(&self, outcome: &SessionOutcome) -> Result<(), EventPublisherError> {
        if let Some(event) = outcome.disconnected {
            self.publisher.publish(event.into())?;
        }
        if let Some(event) = outcome.connected {
            self.publisher.publish(event.into())?;
        }

        Ok(())
    }
}
