use std::sync::Arc;

use shared_types::PayloadKind;

use crate::application::MessageRecorder;
use crate::application::commands::OutboundComposer;
use crate::domain::ConversationId;
use crate::domain::MessageBody;
use crate::ports::{MessageTransportPort, MessagingCommandError, SendOutcome};

/// Say something on the network-wide broadcast channel (D3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishBroadcastMessage {
    pub body: MessageBody,
}

/// Handles [`PublishBroadcastMessage`]: the message is released to the gossip
/// topic and then recorded as published.
///
/// # Published before it is recorded, on purpose
///
/// A broadcast has exactly one delivery state — `Published` — and no failed
/// one, because gossip has no recipient set and no acknowledgement to lose
/// (D3, AC10). There is therefore no honest way to record a broadcast the topic
/// never accepted: leaving it in the conversation would have this peer
/// displaying, as published, something that never left the machine. So the
/// transport goes first and a refusal is an `Err` with nothing recorded.
///
/// # The exact opposite of the direct path
///
/// A direct message is recorded first so that a refused send is *visible* as
/// `Failed(reason)` (AC11). Same two steps, opposite order, for the same
/// reason in both cases: the local record must never claim more than is true.
/// That difference is why these are two commands and not one with a flag.
///
/// # The cost of a refusal
///
/// The sequence number was issued and is now unused, leaving a hole in this
/// peer's broadcast run. Receivers hold what follows for one tolerance window
/// and then close the gap explicitly, naming the abandoned range (rule R,
/// AC15). A visible gap is the right price for never claiming a publication
/// that did not happen.
///
/// # Not confidential, and that is the feature
///
/// Broadcast messages are signed but readable by every member: that is what a
/// network-wide channel is (D3, S8). Only the 1:1 path carries anything
/// private.
#[derive(Clone)]
pub struct PublishBroadcastMessageHandler {
    composer: OutboundComposer,
    transport: Arc<dyn MessageTransportPort + Send + Sync>,
    recorder: MessageRecorder,
}

impl PublishBroadcastMessageHandler {
    pub(crate) const fn new(
        composer: OutboundComposer,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            composer,
            transport,
            recorder,
        }
    }

    pub fn handle(
        &self,
        command: PublishBroadcastMessage,
    ) -> Result<SendOutcome, MessagingCommandError> {
        let conversation = ConversationId::Broadcast;

        let draft =
            self.composer
                .seal(conversation, PayloadKind::BroadcastMessage, &command.body)?;

        self.transport.publish_broadcast(&draft.envelope)?;

        let (sent, message) = self.composer.record(conversation, &draft, command.body)?;
        self.recorder
            .record(std::slice::from_ref(&message), &[sent.into()])?;

        Ok(SendOutcome {
            sent,
            delivery: message.delivery_state(),
        })
    }
}
