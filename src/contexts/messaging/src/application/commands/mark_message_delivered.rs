use std::sync::Arc;

use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::events::MessageDeliveryStateChanged;
use crate::domain::{ConversationError, MessageId};
use crate::ports::MessagingCommandError;

/// Record that a 1:1 message reached its recipient (AC11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkMessageDelivered {
    pub id: MessageId,
}

/// Handles [`MarkMessageDelivered`]: `pending → delivered`, announced.
///
/// # Why an unknown message is an error rather than a shrug
///
/// An acknowledgement for a message this peer does not hold means the two ends
/// disagree about what was sent — a transport bug, a replayed acknowledgement,
/// or a peer answering for traffic it never received. Swallowing it would hide
/// all three. The conversation is not opened to find out: a stray
/// acknowledgement must not be able to populate the conversation list, so a
/// conversation this process has never touched simply has no such message.
///
/// # Terminal states stay terminal
///
/// Marking a message that already failed is refused by the aggregate rather
/// than silently overwritten, so a late acknowledgement can never resurrect a
/// message the user was already told had failed. Broadcast messages are
/// `Published` and refuse this too: gossip has no acknowledgement, so anything
/// claiming one is wrong (D3).
#[derive(Clone)]
pub struct MarkMessageDeliveredHandler {
    registry: Arc<ConversationRegistry>,
    recorder: MessageRecorder,
}

impl MarkMessageDeliveredHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        recorder: MessageRecorder,
    ) -> Self {
        Self { registry, recorder }
    }

    pub fn handle(
        &self,
        command: MarkMessageDelivered,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        let change = self
            .registry
            .modify_open(command.id.conversation(), |open| {
                open.mark_delivered(&command.id)
            })
            .ok_or(ConversationError::UnknownMessage)??;

        self.recorder.announce(change)?;

        Ok(change)
    }
}
