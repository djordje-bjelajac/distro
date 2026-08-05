use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::application::queries::{
    GetConversationHistory, GetConversationHistoryHandler, GetMessageDeliveryState,
    GetMessageDeliveryStateHandler, ListConversations, ListConversationsHandler,
};
use crate::domain::{ConversationId, DeliveryState, Message, MessageId};
use crate::ports::{MessageLogError, MessageLogPort, MessagingQueryPort};

/// The read half of this context's inbound surface: one [`MessagingQueryPort`]
/// implementation over the three query handlers.
///
/// Every method reads and returns; none writes, and none can, because no
/// handler behind it holds a transport, a signer, a counter, or the registry's
/// mutating entry points — it reaches the conversations only through
/// `ConversationRegistry::read`, which never opens one.
///
/// Wired over the same registry as the command services, so a message accepted
/// through `InboundEnvelopePort` is immediately visible here.
#[derive(Clone)]
pub struct MessagingQueryService {
    conversations: ListConversationsHandler,
    history: GetConversationHistoryHandler,
    delivery_state: GetMessageDeliveryStateHandler,
}

impl MessagingQueryService {
    pub fn new(
        registry: Arc<ConversationRegistry>,
        log: Arc<dyn MessageLogPort + Send + Sync>,
    ) -> Self {
        Self {
            conversations: ListConversationsHandler::new(log),
            history: GetConversationHistoryHandler::new(Arc::clone(&registry)),
            delivery_state: GetMessageDeliveryStateHandler::new(registry),
        }
    }
}

impl MessagingQueryPort for MessagingQueryService {
    fn conversations(&self) -> Result<Vec<ConversationId>, MessageLogError> {
        self.conversations.handle(ListConversations)
    }

    fn history(&self, conversation: ConversationId) -> Vec<Message> {
        self.history.handle(GetConversationHistory { conversation })
    }

    fn delivery_state(&self, id: MessageId) -> Option<DeliveryState> {
        self.delivery_state.handle(GetMessageDeliveryState { id })
    }
}
