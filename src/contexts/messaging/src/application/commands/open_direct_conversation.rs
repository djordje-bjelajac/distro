use std::sync::Arc;

use shared_types::PeerId;

use crate::application::ConversationRegistry;
use crate::domain::ConversationId;
use crate::ports::MessagingCommandError;

/// Make sure the 1:1 conversation with one peer exists, with this peer's
/// outbound sequence restored (D12, AC16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenDirectConversation {
    pub peer: PeerId,
}

/// Handles [`OpenDirectConversation`]: rehydrates a conversation from the
/// outbound counter before anyone needs it.
///
/// # What it is for
///
/// History dies with the process, the sequence counter does not (D7, D12). A
/// restarted peer that resumed numbering at 1 would have every message it sent
/// classified a duplicate by listeners still holding its old high-water mark —
/// it would go permanently mute while appearing, to itself, to work. Every
/// conversation is therefore rehydrated from the counter on first touch.
///
/// This command only moves *when* that happens: to the moment a session is
/// established rather than the moment the user presses send. The counter read
/// may touch a store (`infra-store-fs`, OP-11), and doing it while nobody is
/// waiting is the difference between a warm conversation and a stall on the
/// first keystroke.
///
/// It is idempotent by construction — a conversation already open is left
/// exactly as it is — so a flapping session costs nothing.
#[derive(Clone)]
pub struct OpenDirectConversationHandler {
    registry: Arc<ConversationRegistry>,
}

impl OpenDirectConversationHandler {
    pub(crate) const fn new(registry: Arc<ConversationRegistry>) -> Self {
        Self { registry }
    }

    pub fn handle(&self, command: OpenDirectConversation) -> Result<(), MessagingCommandError> {
        self.registry
            .modify(ConversationId::Direct(command.peer), |_| ())
    }
}
