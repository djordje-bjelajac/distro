use std::sync::Arc;

use shared_types::{Compatibility, Envelope, PayloadKind, PeerId};

use crate::application::{ConversationRegistry, MessageRecorder, MessagingSettings};
use crate::domain::events::{MessageRejected, RejectionReason};
use crate::domain::{ConversationId, Message};
use crate::ports::{
    AuthorPolicyPort, ClockPort, EnvelopeVerifierPort, InboundVerdict, MessagePayload,
    MessagingCommandError, VerifiedAuthor,
};

/// Take in one envelope that arrived from the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptInboundMessage {
    pub envelope: Envelope,
}

/// Handles [`AcceptInboundMessage`]: the boundary S3 names, in the order S3
/// names it.
///
/// Everything the network says about messages enters the domain here and
/// nowhere else. Each step below can end the envelope's journey, and each one
/// is placed where it is for a reason:
///
/// 1. **Protocol version.** A different major version is a wire format this
///    build cannot read; there is nothing to check a signature *of*. Rejected
///    with a stated reason (S2, AC14).
/// 2. **Signature.** This is what establishes an author at all (invariant 4).
///    It comes before the block list because "is this peer blocked" is a
///    question about an author, and until the signature verifies the `author`
///    field is a claim anyone could have written. Checking the block list
///    first would let any peer bypass a block by putting someone else's
///    `PeerId` in the field. An invalid signature never reaches a read model
///    (invariant 10, AC6), and a verifier that cannot *run* is an error rather
///    than a verdict — unknown is not valid.
/// 3. **Block list.** A blocked peer's envelopes are dropped at the
///    application boundary of every context (invariant 11). Dropped before the
///    payload is even parsed: refusing to listen means refusing to process, and
///    nothing is announced to the blocked peer.
/// 4. **Payload.** An unreadable payload is refused; a payload *kind* this
///    build does not act on is tolerated and counted instead, because peers
///    upgrade independently and a newer peer's new kind is not an attack (S2,
///    AC14).
/// 5. **The conversation**, with the arrival instant taken from this peer's own
///    clock.
///
/// # The author cannot be faked past this point
///
/// [`Conversation::accept_remote`](crate::domain::Conversation::accept_remote)
/// trusts the author it is given. The private step that calls it takes a
/// [`VerifiedAuthor`], and the only way to obtain one is to run the verifier
/// over a real envelope — so a future edit that tried to reach the conversation
/// with an unverified author would not fail a review, it would fail to compile.
///
/// # Two instants, one of which is a fact
///
/// `received_at` is read from [`ClockPort`] here and never taken from the
/// author's claimed send time. The claim is another peer's clock — unsynchronised
/// and freely falsifiable — and it is `received_at` that ages a gap (rule R).
/// Letting the claim age anything would hand any author the power to hold a gap
/// open indefinitely, or to force one shut, by lying about the time.
#[derive(Clone)]
pub struct AcceptInboundMessageHandler {
    registry: Arc<ConversationRegistry>,
    settings: MessagingSettings,
    verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
    policy: Arc<dyn AuthorPolicyPort + Send + Sync>,
    clock: Arc<dyn ClockPort + Send + Sync>,
    recorder: MessageRecorder,
}

impl AcceptInboundMessageHandler {
    pub(crate) const fn new(
        registry: Arc<ConversationRegistry>,
        settings: MessagingSettings,
        verifier: Arc<dyn EnvelopeVerifierPort + Send + Sync>,
        policy: Arc<dyn AuthorPolicyPort + Send + Sync>,
        clock: Arc<dyn ClockPort + Send + Sync>,
        recorder: MessageRecorder,
    ) -> Self {
        Self {
            registry,
            settings,
            verifier,
            policy,
            clock,
            recorder,
        }
    }

    pub fn handle(
        &self,
        command: AcceptInboundMessage,
    ) -> Result<InboundVerdict, MessagingCommandError> {
        let envelope = command.envelope;

        // 1 — version (S2, AC14).
        if envelope.compatibility(&self.settings.protocol_version) == Compatibility::Reject {
            return self.refuse(&envelope, RejectionReason::UnsupportedProtocolVersion);
        }

        // 2 — signature (invariants 4 and 10, AC6). The only mint site of a
        // `VerifiedAuthor`.
        let Some(author) = VerifiedAuthor::attest(self.verifier.as_ref(), &envelope)? else {
            return self.refuse(&envelope, RejectionReason::SignatureInvalid);
        };

        // 3 — block list (invariant 11).
        if self.policy.is_blocked(author.peer()) {
            return self.refuse(&envelope, RejectionReason::AuthorBlocked);
        }

        // 4 — routing and payload.
        let Some(conversation) = conversation_for(envelope.kind, author.peer()) else {
            return Ok(InboundVerdict::Ignored(envelope.kind));
        };
        let Ok(payload) = MessagePayload::decode(&envelope.payload) else {
            return self.refuse(&envelope, RejectionReason::MalformedPayload);
        };

        // 5 — the domain. Reachable only with an attested author.
        self.accept_verified(author, conversation, payload)
    }

    /// The one path into the conversation, and the reason this handler can
    /// make an invariant-4 promise the domain cannot make for itself.
    ///
    /// Taking `VerifiedAuthor` **by value** is the safeguard: the value cannot
    /// be cloned, copied, or constructed anywhere but
    /// [`VerifiedAuthor::attest`], so reaching this function at all is proof
    /// that a signature verified for this envelope's author.
    fn accept_verified(
        &self,
        author: VerifiedAuthor,
        conversation: ConversationId,
        payload: MessagePayload,
    ) -> Result<InboundVerdict, MessagingCommandError> {
        // Read before the lock is taken, and from this peer's clock alone.
        let received_at = self.clock.now();
        let (sequence, claimed_sent_at, body) = payload.into_parts();
        let author = author.into_peer();

        let judged = self.registry.modify(conversation, move |open| {
            let outcome =
                open.accept_remote(author, sequence, body, claimed_sent_at, received_at)?;

            let placement = outcome.placement().clone();
            // What became visible is read back out of the conversation: the
            // events name the messages, the conversation holds them, and only
            // applied messages are ever mirrored.
            let applied: Vec<Message> = outcome
                .applied()
                .iter()
                .filter_map(|event| open.message(&event.id).cloned())
                .collect();

            Ok::<_, MessagingCommandError>((placement, applied, outcome.into_events()))
        })?;

        let (placement, applied, events) = judged?;
        self.recorder.record(&applied, &events)?;

        Ok(InboundVerdict::Judged(placement))
    }

    /// Refuses an envelope at the boundary, announcing why (AC6, AC14, AC15).
    ///
    /// The rejection carries no sequence number. Every refusal here happens
    /// either before an author is established — when nothing in the envelope is
    /// trustworthy, including the number — or before the payload is parsed, so
    /// there is no number to report. Inventing one from unverified bytes would
    /// put an attacker's choice of sequence into this peer's diagnostics.
    fn refuse(
        &self,
        envelope: &Envelope,
        reason: RejectionReason,
    ) -> Result<InboundVerdict, MessagingCommandError> {
        let rejected = MessageRejected {
            conversation: claimed_conversation(envelope),
            claimed_author: envelope.author,
            sequence: None,
            reason,
        };

        self.recorder.announce(rejected)?;

        Ok(InboundVerdict::RefusedAtBoundary(rejected))
    }
}

/// Which conversation a verified author's envelope belongs to; `None` for a
/// payload kind this context does not act on.
///
/// A direct message from `author` belongs to *this peer's* conversation with
/// that author — `Direct` is identified by the counterpart, and from the
/// receiving end the counterpart is the sender.
const fn conversation_for(kind: PayloadKind, author: PeerId) -> Option<ConversationId> {
    match kind {
        PayloadKind::BroadcastMessage => Some(ConversationId::Broadcast),
        PayloadKind::DirectMessage => Some(ConversationId::Direct(author)),
        // A heartbeat is `membership`'s evidence of life, not a message; an
        // unknown kind is a newer peer speaking (S2). Neither is a refusal.
        PayloadKind::Heartbeat | PayloadKind::Unknown(_) => None,
    }
}

/// Where an envelope *claims* to belong, for a rejection raised before its
/// author was established.
fn claimed_conversation(envelope: &Envelope) -> ConversationId {
    match envelope.kind {
        PayloadKind::BroadcastMessage => ConversationId::Broadcast,
        _ => ConversationId::Direct(envelope.author),
    }
}
