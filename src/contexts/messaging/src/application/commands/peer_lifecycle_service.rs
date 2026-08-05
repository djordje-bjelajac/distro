use std::sync::Arc;

use shared_types::{PeerConnected, PeerDisconnected};

use crate::application::commands::{
    FailPendingDirects, FailPendingDirectsHandler, OpenDirectConversation,
    OpenDirectConversationHandler,
};
use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::DeliveryFailure;
use crate::domain::events::MessageDeliveryStateChanged;
use crate::ports::{MessagingCommandError, PeerLifecyclePort};

/// The lifecycle half of this context's inbound surface: one
/// [`PeerLifecyclePort`] implementation over the two handlers that react to a
/// session appearing and disappearing (D10).
///
/// # The only seam between two contexts
///
/// `membership` publishes `PeerConnected` / `PeerDisconnected` through
/// `shared_types`, carrying a `PeerId` and nothing else; the composition root
/// hands them here. Neither context imports the other, and this one still never
/// learns what an `Endpoint`, a session, or a reachability class is (canvas §4).
///
/// # Symmetry that is not accidental
///
/// A connect prepares the conversation this peer will speak in — restoring its
/// outbound sequence before anyone waits on it (D12, AC16). A disconnect closes
/// out what that conversation still had in flight (AC11). Both are about one
/// peer's direct conversation and neither touches the broadcast channel, which
/// has no session to gain or lose (D3).
#[derive(Clone)]
pub struct PeerLifecycleService {
    open_conversation: OpenDirectConversationHandler,
    fail_pending: FailPendingDirectsHandler,
}

impl PeerLifecycleService {
    pub(crate) fn new(registry: Arc<ConversationRegistry>, recorder: MessageRecorder) -> Self {
        Self {
            open_conversation: OpenDirectConversationHandler::new(Arc::clone(&registry)),
            fail_pending: FailPendingDirectsHandler::new(registry, recorder),
        }
    }
}

impl PeerLifecyclePort for PeerLifecycleService {
    fn peer_connected(&self, event: PeerConnected) -> Result<(), MessagingCommandError> {
        self.open_conversation
            .handle(OpenDirectConversation { peer: event.peer })
    }

    fn peer_disconnected(
        &self,
        event: PeerDisconnected,
    ) -> Result<Vec<MessageDeliveryStateChanged>, MessagingCommandError> {
        self.fail_pending.handle(FailPendingDirects {
            peer: event.peer,
            // The session is what died, so that is what the user is told. Any
            // vaguer reason would leave them unable to decide between resending
            // now and waiting for the peer to come back (AC11).
            reason: DeliveryFailure::SessionClosed,
        })
    }
}
