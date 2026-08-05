use std::sync::Arc;

use shared_types::PeerId;

use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::events::MessageDeliveryStateChanged;
use crate::domain::{ConversationId, DeliveryFailure};
use crate::ports::MessagingCommandError;

/// Give up on every 1:1 message to one peer that is still awaiting
/// acknowledgement (D10, AC11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailPendingDirects {
    /// Whose conversation to fail. Only this peer's — a disconnect is news
    /// about one link, not about the network.
    pub peer: PeerId,
    pub reason: DeliveryFailure,
}

/// Handles [`FailPendingDirects`]: what a `PeerDisconnected` costs.
///
/// A direct message handed to a transport whose session has died will not
/// arrive. AC11 makes silent loss a non-state, so each one ends in a stated
/// failure the user can act on and resend from — rather than sitting at
/// `pending` forever, which is silent loss wearing a spinner.
///
/// # The aggregate decides which messages those are
///
/// `Conversation::fail_pending` walks its own state. A handler iterating
/// identifiers from outside would be reimplementing that walk against a read
/// view that may already have moved, and would have to know that broadcast
/// messages are `Published` rather than pending and that terminal states are
/// terminal. All of that is the aggregate's knowledge.
///
/// # A conversation that was never opened has nothing pending
///
/// A disconnect from a peer this instance never messaged opens nothing:
/// lifecycle news must not be able to populate the conversation list.
#[derive(Clone)]
pub struct FailPendingDirectsHandler {
    registry: Arc<ConversationRegistry>,
    recorder: MessageRecorder,
}

impl FailPendingDirectsHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        recorder: MessageRecorder,
    ) -> Self {
        Self { registry, recorder }
    }

    pub fn handle(
        &self,
        command: FailPendingDirects,
    ) -> Result<Vec<MessageDeliveryStateChanged>, MessagingCommandError> {
        let changes = self
            .registry
            .modify_open(ConversationId::Direct(command.peer), |open| {
                open.fail_pending(command.reason)
            })
            .unwrap_or_default();

        for change in &changes {
            self.recorder.announce(*change)?;
        }

        Ok(changes)
    }
}
