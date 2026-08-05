use std::sync::Arc;

use shared_types::{Envelope, PayloadKind};

use crate::application::{ConversationRegistry, MessagingSettings};
use crate::domain::events::MessageSent;
use crate::domain::{
    ConversationError, ConversationId, Message, MessageBody, Millis, SequenceNumber,
};
use crate::ports::{
    ClockPort, EnvelopeSignerPort, MessagePayload, MessagingCommandError, SequenceCounterPort,
    UnsignedEnvelope,
};

/// The part of composing an outbound message that is identical whichever
/// conversation it is for: take a number, stamp an instant, sign an envelope,
/// and record the message locally.
///
/// It exists so the two send paths stay about what actually differs between
/// them — a direct message is acknowledged and can fail visibly (D4, D10,
/// AC11), a broadcast is released and can only ever be `Published` (D3, AC10) —
/// rather than about drafting mechanics. The order in which the steps below are
/// used is *not* shared: each handler chooses it, and that choice is the whole
/// of the difference.
///
/// The two steps are separate for exactly that reason.
/// [`seal`](Self::seal) produces something sendable without touching the
/// conversation; [`record`](Self::record) makes it locally real. A direct send
/// records first, so AC11's failed message exists to be seen; a broadcast
/// publishes first, so nothing local claims a publication that never happened.
#[derive(Clone)]
pub(crate) struct OutboundComposer {
    registry: Arc<ConversationRegistry>,
    settings: MessagingSettings,
    clock: Arc<dyn ClockPort + Send + Sync>,
    counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
}

/// An envelope that is signed and sendable, and the two facts the local record
/// must agree with.
pub(crate) struct SealedDraft {
    /// Signed, and therefore attributable to this peer (invariant 4).
    pub(crate) envelope: Envelope,
    /// The number the counter issued (D12).
    pub(crate) sequence: SequenceNumber,
    /// This peer's clock reading, carried on the wire as the author's claim.
    pub(crate) claimed_sent_at: Millis,
}

impl OutboundComposer {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        settings: MessagingSettings,
        clock: Arc<dyn ClockPort + Send + Sync>,
        counter: Arc<dyn SequenceCounterPort + Send + Sync>,
        signer: Arc<dyn EnvelopeSignerPort + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            settings,
            clock,
            counter,
            signer,
        }
    }

    /// Issues a sequence number, drafts an envelope around the body, and signs
    /// it — without touching any conversation.
    ///
    /// # The number comes from the counter
    ///
    /// Not from the conversation's own mark. History dies with the process
    /// (D7); the counter does not, because its true domain of validity is the
    /// identity rather than the process (D12). A peer that resumed numbering at
    /// 1 after a restart would have every message it sent classified a
    /// duplicate by listeners still holding its old high-water mark — mute,
    /// while appearing to itself to work. `issue_next` records the advance
    /// before it returns, so a number that reaches the wire is a number that
    /// survives a crash (AC16).
    ///
    /// # Signing before recording
    ///
    /// A signer that cannot sign leaves no local trace of a message that never
    /// existed. The cost is a number issued and never used, which is a gap in
    /// this author's run — and a gap is *visible*: receivers wait one tolerance
    /// window and then report it (rule R, AC15). That is strictly better than
    /// the alternative, a locally displayed message that will never be sent.
    pub(crate) fn seal(
        &self,
        conversation: ConversationId,
        kind: PayloadKind,
        body: &MessageBody,
    ) -> Result<SealedDraft, MessagingCommandError> {
        // Opened *before* the counter advances, and this order is load-bearing.
        // Opening a conversation rehydrates its local mark from the counter, so
        // a conversation first touched after `issue_next` would come back
        // already holding this message's number and every send would look like
        // a divergence. Doing it here also means a counter that cannot be read
        // stops the send before anything else happens.
        self.registry.modify(conversation, |_| ())?;

        let claimed_sent_at = self.clock.now();
        let sequence = self.counter.issue_next(conversation)?;
        let payload = MessagePayload::new(sequence, claimed_sent_at, body.clone());

        let envelope = self.signer.seal(UnsignedEnvelope::draft(
            self.settings.local_peer,
            self.settings.protocol_version,
            kind,
            payload.encode(),
        ))?;

        Ok(SealedDraft {
            envelope,
            sequence,
            claimed_sent_at,
        })
    }

    /// Appends the message to its conversation, returning the event and the
    /// message itself.
    ///
    /// # The guard
    ///
    /// The conversation derives the next number from its own mark, and the
    /// counter issued one independently. They agree by construction — every
    /// conversation is rehydrated from the counter on first touch — and the
    /// check is here anyway, before anything is appended, because the failure
    /// it catches is invisible to the sender and corrupting for every receiver:
    /// a message whose wire sequence differs from the one its local identifier
    /// carries.
    pub(crate) fn record(
        &self,
        conversation: ConversationId,
        draft: &SealedDraft,
        body: MessageBody,
    ) -> Result<(MessageSent, Message), MessagingCommandError> {
        let local = self.registry.local_peer();
        let issued = draft.sequence;
        let claimed_sent_at = draft.claimed_sent_at;

        self.registry.modify(conversation, move |open| {
            let next = SequenceNumber::following(open.high_water_mark(&local))
                .map_err(ConversationError::from)?;
            if next != issued {
                return Err(MessagingCommandError::SequenceDiverged {
                    issued,
                    recorded: next,
                });
            }

            let sent = open.append_local(body, claimed_sent_at)?;
            let message = open
                .message(&sent.id)
                .cloned()
                .expect("a message just appended locally is applied");

            Ok((sent, message))
        })?
    }
}
