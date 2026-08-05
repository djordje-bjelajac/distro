use std::sync::Arc;

use shared_types::Envelope;

use crate::application::commands::{
    AcceptInboundMessage, AcceptInboundMessageHandler, CloseAgedGaps, CloseAgedGapsHandler,
    MarkMessageDelivered, MarkMessageDeliveredHandler,
};
use crate::application::{ConversationRegistry, MessageRecorder, MessagingSettings};
use crate::domain::MessageId;
use crate::domain::events::{MessageDeliveryStateChanged, MessageGapClosed};
use crate::ports::{
    AuthorPolicyPort, ClockPort, EnvelopeVerifierPort, InboundEnvelopePort, InboundVerdict,
    MessagingCommandError,
};

/// The report half of this context's inbound surface: one
/// [`InboundEnvelopePort`] implementation over the handlers the network runtime
/// and its clock tick drive (S3).
///
/// Every method here is a *report* — an envelope arrived, a recipient
/// acknowledged, a tolerance window elapsed — never a decision. The decisions
/// are [`SendMessagePort`](crate::ports::SendMessagePort)'s.
///
/// The three belong together because they are the three ways an inbound story
/// ends: the message arrived, the acknowledgement arrived, or nothing arrived
/// and the wait is over.
#[derive(Clone)]
pub struct InboundEnvelopeService {
    accept: AcceptInboundMessageHandler,
    delivered: MarkMessageDeliveredHandler,
    close_gaps: CloseAgedGapsHandler,
}

impl InboundEnvelopeService {
    pub(crate) fn new(
        registry: Arc<ConversationRegistry>,
        settings: MessagingSettings,
        clock: Arc<dyn ClockPort + Send + Sync>,
        verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
        policy: Arc<dyn AuthorPolicyPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            accept: AcceptInboundMessageHandler::new(
                Arc::clone(&registry),
                settings,
                verifier,
                policy,
                Arc::clone(&clock),
                recorder.clone(),
            ),
            delivered: MarkMessageDeliveredHandler::new(Arc::clone(&registry), recorder.clone()),
            close_gaps: CloseAgedGapsHandler::new(registry, settings, clock, recorder),
        }
    }
}

impl InboundEnvelopePort for InboundEnvelopeService {
    fn accept_envelope(&self, envelope: Envelope) -> Result<InboundVerdict, MessagingCommandError> {
        self.accept.handle(AcceptInboundMessage { envelope })
    }

    fn message_delivered(
        &self,
        id: MessageId,
    ) -> Result<MessageDeliveryStateChanged, MessagingCommandError> {
        self.delivered.handle(MarkMessageDelivered { id })
    }

    fn close_aged_gaps(&self) -> Result<Vec<MessageGapClosed>, MessagingCommandError> {
        self.close_gaps.handle(CloseAgedGaps)
    }
}
