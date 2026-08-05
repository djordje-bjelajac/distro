use std::sync::Arc;

use shared_types::{PayloadKind, PeerId};

use crate::application::commands::OutboundComposer;
use crate::application::{ConversationRegistry, MessageRecorder};
use crate::domain::ConversationId;
use crate::domain::MessageBody;
use crate::domain::events::MessagingEvent;
use crate::ports::{MessageTransportPort, MessagingCommandError, SendOutcome};

/// Send a 1:1 message to one peer (D4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendDirectMessage {
    /// Who it is for. A `PeerId` and nothing else — this context never learns
    /// how that peer is reached (canvas §4).
    pub to: PeerId,
    pub body: MessageBody,
}

/// Handles [`SendDirectMessage`]: the message becomes real locally, then goes
/// to the transport, and whichever of those the transport does with it is
/// visible afterwards.
///
/// # Recorded before it is sent, on purpose
///
/// AC11 makes silent loss a non-state: a direct message must carry a delivery
/// state the user can read, and `failed` is one of the two endings. A handler
/// that only recorded on success would have nothing to mark failed — the
/// message would simply never have existed, which is precisely the silent loss
/// AC11 forbids. So the conversation gets it first, at `Pending`, and a
/// transport refusal moves it to `Failed(reason)` through
/// [`MessageTransportError::as_delivery_failure`](crate::ports::MessageTransportError::as_delivery_failure).
///
/// That is also why a refused send returns `Ok`. The command did what it
/// promised — there is a message, and it says what happened to it. Returning
/// `Err` would throw away the record AC11 exists to require.
///
/// # This is the opposite order from the broadcast path
///
/// Deliberately. A broadcast has no failed state at all, so publishing must
/// succeed *before* anything local claims it happened. The two paths differ in
/// exactly this, which is why they are two commands.
///
/// # One attempt, no queue
///
/// The bounded retry cycle of D10 belongs to the transport adapter, which is
/// the only layer that knows a session is still alive. This handler asks once
/// and reports the answer; it never queues for later delivery, because
/// store-and-forward is excluded from v1 and a hidden queue would make
/// `pending` mean "maybe, eventually" instead of "handed over, unacknowledged".
#[derive(Clone)]
pub struct SendDirectMessageHandler {
    registry: Arc<ConversationRegistry>,
    composer: OutboundComposer,
    transport: Arc<dyn MessageTransportPort + Send + Sync>,
    recorder: MessageRecorder,
}

impl SendDirectMessageHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        composer: OutboundComposer,
        transport: Arc<dyn MessageTransportPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            registry,
            composer,
            transport,
            recorder,
        }
    }

    pub fn handle(&self, command: SendDirectMessage) -> Result<SendOutcome, MessagingCommandError> {
        let conversation = ConversationId::Direct(command.to);

        let draft = self
            .composer
            .seal(conversation, PayloadKind::DirectMessage, &command.body)?;
        let (sent, message) = self.composer.record(conversation, &draft, command.body)?;

        let mut delivery = message.delivery_state();
        let mut events = vec![MessagingEvent::from(sent)];

        if let Err(error) = self.transport.send_direct(command.to, &draft.envelope) {
            // Visible, never silent (D10, AC11). The aggregate owns the
            // transition, so the reason the transport gave is translated into a
            // delivery meaning and handed to it.
            let change = self.registry.modify(conversation, |open| {
                open.mark_failed(&sent.id, error.as_delivery_failure())
            })??;

            delivery = change.to;
            events.push(change.into());
        }

        self.recorder.record(&[message], &events)?;

        Ok(SendOutcome { sent, delivery })
    }
}
