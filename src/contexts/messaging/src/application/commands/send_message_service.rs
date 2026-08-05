use std::sync::Arc;

use shared_types::PeerId;

use crate::application::commands::{
    OutboundComposer, PublishBroadcastMessage, PublishBroadcastMessageHandler, SendDirectMessage,
    SendDirectMessageHandler,
};
use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::MessageBody;
use crate::ports::{MessageTransportPort, MessagingCommandError, SendMessagePort, SendOutcome};

/// The compose half of this context's inbound surface: one
/// [`SendMessagePort`] implementation over the two send handlers.
///
/// It holds handlers rather than reimplementing them, so each path keeps its
/// own file, its own tests, and its own ordering decision — the two are not
/// variations of one operation, and the canvas requires they stay separate.
/// This type adds only the translation from the port's domain-typed arguments
/// to the imperative command DTOs, and contains no decision of its own.
///
/// Nothing here reads for display; that is
/// [`MessagingQueryService`](crate::application::queries::MessagingQueryService).
#[derive(Clone)]
pub struct SendMessageService {
    send_direct: SendDirectMessageHandler,
    publish_broadcast: PublishBroadcastMessageHandler,
}

impl SendMessageService {
    /// Wires both paths over one registry, one composer, and one recorder.
    ///
    /// Assembled by [`MessagingContext`](crate::application::MessagingContext),
    /// which is what guarantees the composer here draws from the same counter
    /// the registry rehydrates from (D12).
    pub(crate) fn new(
        registry: Arc<ConversationRegistry>,
        composer: OutboundComposer,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            send_direct: SendDirectMessageHandler::new(
                registry,
                composer.clone(),
                Arc::clone(&transport),
                recorder.clone(),
            ),
            publish_broadcast: PublishBroadcastMessageHandler::new(composer, transport, recorder),
        }
    }
}

impl SendMessagePort for SendMessageService {
    fn send_direct(
        &self,
        to: PeerId,
        body: MessageBody,
    ) -> Result<SendOutcome, MessagingCommandError> {
        self.send_direct.handle(SendDirectMessage { to, body })
    }

    fn publish_broadcast(&self, body: MessageBody) -> Result<SendOutcome, MessagingCommandError> {
        self.publish_broadcast
            .handle(PublishBroadcastMessage { body })
    }
}
