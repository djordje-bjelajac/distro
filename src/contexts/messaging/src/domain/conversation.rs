use std::collections::BTreeMap;
use std::fmt;

use shared_types::PeerId;

use crate::domain::events::{
    GapCloseCause, MessageDeliveryStateChanged, MessageDuplicateIgnored, MessageGapClosed,
    MessageReceived, MessageRejected, MessageSent, RejectionReason,
};
use crate::domain::{
    AuthorLog, ConversationId, DeliveryFailure, DeliveryState, DeliveryStateError, DurationMillis,
    InboundOutcome, Message, MessageBody, MessageId, MessagePlacement, Millis, SequenceNumber,
    SequenceNumberError,
};

/// One conversation as this peer sees it: the aggregate root of the `messaging`
/// context (canvas §2.3).
///
/// # This view is local, and only local
///
/// Invariant 9: what this peer has applied is authoritative for this peer
/// alone. Two participants' conversations routinely differ — one has messages
/// the other has not received yet, and on the broadcast channel a late joiner
/// never sees what was said before it arrived (AC10). That is correct, not a
/// convergence bug.
///
/// # Ordering is per author, and comes from sequence numbers
///
/// Each author gets an [`AuthorLog`]: their applied run plus a bounded buffer of
/// messages that arrived early. Nothing orders across authors, because nothing
/// could: there is no global clock, no consensus, and an author's claimed send
/// time is theirs to invent. AC8 asks only that one author's messages display
/// in that author's send order, and sequence numbers decide that alone
/// (invariant 5).
///
/// # A gap waits, but not forever (rule R)
///
/// An out-of-order arrival is held until the run leading to it completes.
/// Waiting without end would be its own kind of loss: a late joiner's first
/// sighting of an author is almost never sequence 1, and a message that fell
/// off the network permanently would block everything that author says
/// afterwards until the buffer overflowed. So a gap is bounded twice over —
/// by [`GAP_TOLERANCE`](Self::GAP_TOLERANCE), swept through
/// [`close_aged_gaps`](Self::close_aged_gaps), and by
/// [`AuthorLog::MAX_BUFFERED_MESSAGES`] — and when either bound is reached the
/// log moves past the gap and says so with a
/// [`MessageGapClosed`] naming the abandoned range (AC15). Content is never
/// dropped silently and never shown out of its author's order.
///
/// [`SequenceNumber::FIRST`] is the exception that needs no wait: genesis is
/// provable from the number itself, so first contact at sequence 1 applies
/// immediately and first-contact latency (AC1, AC2) is untouched.
///
/// Adopting the *first observed* sequence as a baseline instead would be
/// cheaper and wrong: a hostile — or merely faster — peer could pin a fresh
/// joiner's baseline high and every honest message below it would be discarded
/// in silence.
///
/// # Ports are absent on purpose
///
/// Nothing here reads a clock, signs anything, or touches a transport. Both
/// instants are passed in (D11, S5), and every consequence a transition has
/// beyond the conversation's own state comes back as an event or an
/// [`InboundOutcome`] for the application to carry out. There is no `Endpoint`
/// here and none anywhere in this crate: peers are addressed by `PeerId` only
/// (canvas §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    id: ConversationId,
    local: PeerId,
    authors: BTreeMap<PeerId, AuthorLog>,
}

impl Conversation {
    /// How long a gap may stay open before the log gives up on it (S6).
    ///
    /// 2 seconds, chosen to sit in the band between two thresholds: it is well
    /// above the spread a gossip mesh introduces reordering a message across a
    /// few wide-area hops, so ordinary reordering resolves inside the window and
    /// raises no diagnostic; and it is below the point at which a person
    /// watching a conversation stall concludes the application is broken. A
    /// shorter window would abandon messages that were merely in flight; a
    /// longer one would let one missing message silence an author for as long
    /// as a user is willing to wait, which is what AC10 forbids.
    ///
    /// It is a default, not a rule: every caller passes the window it wants
    /// into [`close_aged_gaps`](Self::close_aged_gaps), so tests and
    /// deployments can differ without touching the aggregate.
    pub const GAP_TOLERANCE: DurationMillis = DurationMillis::from_millis(2_000);

    /// The network-wide broadcast channel, as seen by `local` (D3).
    pub const fn broadcast(local: PeerId) -> Self {
        Self {
            id: ConversationId::Broadcast,
            local,
            authors: BTreeMap::new(),
        }
    }

    /// The 1:1 conversation between `local` and `peer` (D4).
    pub fn direct(local: PeerId, peer: PeerId) -> Result<Self, ConversationError> {
        if local == peer {
            return Err(ConversationError::SelfConversation);
        }

        Ok(Self {
            id: ConversationId::Direct(peer),
            local,
            authors: BTreeMap::new(),
        })
    }

    /// Rebuilds a conversation whose local author has already issued sequence
    /// numbers up to `local_high_water` (D12, AC16).
    ///
    /// History does not survive the process (D7) but the outbound counter does,
    /// because its true domain of validity is the identity rather than the
    /// process: a peer that resumed at 1 after a restart would be re-sending
    /// numbers its listeners already hold, and every message it sent would be
    /// classified a duplicate — going permanently mute while appearing, to
    /// itself, to work. `local_high_water` comes from
    /// [`SequenceCounterPort`](crate::ports::SequenceCounterPort).
    ///
    /// The rebuilt conversation holds no messages. The mark records what this
    /// peer has *issued*, never what it holds, which is precisely why
    /// [`AuthorLog::is_applied`] tests membership.
    pub fn rehydrate(
        id: ConversationId,
        local: PeerId,
        local_high_water: Option<SequenceNumber>,
    ) -> Result<Self, ConversationError> {
        if id.counterpart() == Some(local) {
            return Err(ConversationError::SelfConversation);
        }

        let mut authors = BTreeMap::new();
        if let Some(high_water) = local_high_water {
            authors.insert(local, AuthorLog::rehydrated(local, high_water));
        }

        Ok(Self { id, local, authors })
    }

    pub const fn id(&self) -> ConversationId {
        self.id
    }

    /// The peer this view belongs to.
    pub const fn local_peer(&self) -> PeerId {
        self.local
    }

    /// Every author's log, in `PeerId` order, so iteration is deterministic
    /// (AC13).
    pub fn logs(&self) -> impl Iterator<Item = &AuthorLog> {
        self.authors.values()
    }

    /// One author's log, if that author has ever been seen here.
    pub fn log(&self, author: &PeerId) -> Option<&AuthorLog> {
        self.authors.get(author)
    }

    /// One author's applied messages, in their send order; empty for an author
    /// with nothing visible yet.
    pub fn messages_by(&self, author: &PeerId) -> &[Message] {
        self.authors.get(author).map_or(&[], |log| log.messages())
    }

    /// An applied message by identifier. Buffered messages are unreachable —
    /// they are not part of the conversation yet — and so are abandoned ones,
    /// which never will be (invariant 5, rule R).
    pub fn message(&self, id: &MessageId) -> Option<&Message> {
        if id.conversation() != self.id {
            return None;
        }

        self.authors
            .get(&id.author())
            .and_then(|log| log.applied(id.sequence()))
    }

    /// How far one author's applied run reaches; `None` before their first
    /// message.
    pub fn high_water_mark(&self, author: &PeerId) -> Option<SequenceNumber> {
        self.authors
            .get(author)
            .and_then(AuthorLog::high_water_mark)
    }

    /// How many of one author's messages are held waiting for a gap to close.
    pub fn buffered_count(&self, author: &PeerId) -> usize {
        self.authors
            .get(author)
            .map_or(0, AuthorLog::buffered_count)
    }

    /// Total applied messages across all authors.
    pub fn applied_len(&self) -> usize {
        self.authors.values().map(|log| log.messages().len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.applied_len() == 0
    }

    /// Appends a message this peer composed, assigning it the next sequence
    /// number for the local author.
    ///
    /// The local author's run is contiguous by construction — this is the only
    /// thing that extends it — so a locally composed message never buffers and
    /// never duplicates.
    ///
    /// `claimed_sent_at` comes from `ClockPort`; it is recorded for display and
    /// plays no part in ordering.
    pub fn append_local(
        &mut self,
        body: MessageBody,
        claimed_sent_at: Millis,
    ) -> Result<MessageSent, ConversationError> {
        let local = self.local;
        let sequence = SequenceNumber::following(self.high_water_mark(&local))?;
        let id = MessageId::new(local, self.id, sequence);

        self.log_mut(local)
            .apply(Message::outbound(id, body, claimed_sent_at));

        Ok(MessageSent {
            id,
            claimed_sent_at,
        })
    }

    /// Takes in a message from another peer.
    ///
    /// # Precondition: `author` is already verified
    ///
    /// This method **trusts** `author`. Invariant 4 says a message's author is
    /// the `PeerId` whose signature verified on the envelope, never a payload
    /// field — and verification is a port operation, which the domain cannot
    /// perform and must not fake. The application's `AcceptInboundMessage`
    /// handler therefore calls `EnvelopeVerifierPort` first and only reaches
    /// this method with the envelope's *verified* author, having also applied
    /// the version and block-list checks (S3, invariants 10 and 11, the latter
    /// through [`AuthorPolicyPort`](crate::ports::AuthorPolicyPort)). Calling it
    /// with an unverified author would put unauthenticated content into the
    /// read model, which invariant 10 forbids; nothing in this signature can
    /// prevent that, which is why it is stated here.
    ///
    /// # Two instants, and only one of them is a fact
    ///
    /// `claimed_sent_at` is the *author's* claim, kept for display. `received_at`
    /// is this peer's own clock reading for the arrival, and it is the only one
    /// that may drive a rule: it is what ages a gap (rule R). Letting the claim
    /// age anything would hand an author the ability to hold a gap open
    /// indefinitely, or to force one shut, by lying about the time.
    ///
    /// # Rule R, in order
    ///
    /// 1–2. Already applied, or already held → a duplicate that changes
    ///    nothing (invariant 6).
    /// 3. Below the log's origin, or inside a gap it has closed behind →
    ///    [`RejectionReason::ArrivedAfterGapClosed`]. Never a duplicate: this
    ///    is loss, and calling it a duplicate would hide it (AC15).
    /// 4–6. Contiguous — which at first contact means exactly
    ///    [`SequenceNumber::FIRST`], since genesis needs no proof — → applied,
    ///    together with everything it unblocks.
    /// 7. Otherwise held, until the gap closes or is abandoned.
    ///
    /// Every result is an [`InboundOutcome`] rather than a silent effect. `Err`
    /// is reserved for a caller mistake: a message that does not belong in this
    /// conversation at all.
    pub fn accept_remote(
        &mut self,
        author: PeerId,
        sequence: SequenceNumber,
        body: MessageBody,
        claimed_sent_at: Millis,
        received_at: Millis,
    ) -> Result<InboundOutcome, ConversationError> {
        if author == self.local {
            return Err(ConversationError::SelfAuthoredInbound);
        }
        if self.id.counterpart().is_some_and(|peer| peer != author) {
            return Err(ConversationError::AuthorNotInConversation);
        }

        let id = MessageId::new(author, self.id, sequence);
        let conversation = self.id;
        let log = self.log_mut(author);

        // R.1, R.2
        if log.is_applied(sequence) || log.is_buffered(sequence) {
            return Ok(InboundOutcome::new(
                MessagePlacement::DuplicateIgnored(MessageDuplicateIgnored { id }),
                Vec::new(),
                None,
            ));
        }

        Self::place(
            log,
            conversation,
            Message::received(id, body, claimed_sent_at),
            received_at,
        )
    }

    /// Gives up on every gap that has stayed open for at least `tolerance`,
    /// reporting each abandoned range in `PeerId` order (rule R, AC13, AC15).
    ///
    /// A gap ages from the **local** arrival of the oldest message stuck behind
    /// it: that message is the one that has been unreadable the longest, and a
    /// later arrival must not extend its wait. Closing the gap moves the log
    /// past the missing run and makes everything held contiguous with it
    /// visible, in the author's send order.
    ///
    /// Pure and time-parameterised: `now` is a `ClockPort` reading the caller
    /// took (D11, S5), so this is decidable in a test without a clock at all.
    /// Calling it again with nothing new to abandon does nothing and reports
    /// nothing.
    ///
    /// The messages the close *released* are not in the returned events — they
    /// are in the conversation, from each event's `to + 1` onwards. An
    /// application that mirrors applied messages into a read model re-reads
    /// them from there; the event says which author and from where.
    pub fn close_aged_gaps(
        &mut self,
        now: Millis,
        tolerance: DurationMillis,
    ) -> Vec<MessageGapClosed> {
        let conversation = self.id;
        let mut closed = Vec::new();

        for log in self.authors.values_mut() {
            let Some(oldest) = log.oldest_buffered_at() else {
                continue;
            };
            if now.saturating_elapsed_since(oldest) < tolerance {
                continue;
            }
            let Some((from, to)) = log.close_gap() else {
                continue;
            };

            closed.push(MessageGapClosed {
                conversation,
                author: log.author(),
                from,
                to,
                cause: GapCloseCause::ToleranceElapsed,
            });
        }

        closed
    }

    /// Fails every message still awaiting acknowledgement, reporting each
    /// transition (D10, AC11).
    ///
    /// This is what a `PeerDisconnected` costs: a direct message handed to a
    /// transport that no longer has a session will not arrive, and AC11 makes
    /// silent loss a non-state, so each one ends in a stated failure the user
    /// can act on. Broadcast messages are `Published` rather than pending —
    /// gossip has no acknowledgement to lose (D3) — and are left alone, as is
    /// anything already delivered or already failed.
    ///
    /// The decision belongs here rather than in a handler loop: which messages
    /// are still pending is the aggregate's own knowledge, and a caller
    /// iterating identifiers from outside would be reimplementing it against a
    /// read view that may already have moved.
    pub fn fail_pending(&mut self, reason: DeliveryFailure) -> Vec<MessageDeliveryStateChanged> {
        let mut changes = Vec::new();

        for log in self.authors.values_mut() {
            for message in log.applied_messages_mut() {
                if message.delivery_state().is_pending() {
                    changes.push(
                        message
                            .mark_failed(reason)
                            .expect("a pending message may always fail"),
                    );
                }
            }
        }

        changes
    }

    /// Records that a direct message reached its recipient (AC11).
    pub fn mark_delivered(
        &mut self,
        id: &MessageId,
    ) -> Result<MessageDeliveryStateChanged, ConversationError> {
        Ok(self.applied_mut(id)?.mark_delivered()?)
    }

    /// Records that a direct message will not arrive, and why (D10, AC11).
    pub fn mark_failed(
        &mut self,
        id: &MessageId,
        reason: DeliveryFailure,
    ) -> Result<MessageDeliveryStateChanged, ConversationError> {
        Ok(self.applied_mut(id)?.mark_failed(reason)?)
    }

    /// Decides where one non-duplicate arrival belongs (rule R, steps 3–7).
    ///
    /// Takes the log rather than `&mut self` so the whole decision runs against
    /// one author's state, and so the buffer-full close below can re-judge the
    /// arrival without a second borrow of the aggregate.
    fn place(
        log: &mut AuthorLog,
        conversation: ConversationId,
        message: Message,
        received_at: Millis,
    ) -> Result<InboundOutcome, ConversationError> {
        let id = message.id();
        let sequence = message.sequence();
        // Taken before anything moves, so the tail past it is exactly what this
        // call made visible — including a run released by the close below.
        let visible_before = log.messages().len();

        // R.3
        if log.is_out_of_reach(sequence) {
            return Ok(InboundOutcome::new(
                Self::rejection(conversation, log.author(), sequence),
                Vec::new(),
                None,
            ));
        }

        let mut closed_gap = None;
        let mut expected = SequenceNumber::following(log.high_water_mark())?;

        // The second trigger for the same close the tolerance sweep performs:
        // the buffer is full, so rather than refuse this message the oldest gap
        // is given up on and everything held becomes visible (S6, AC15).
        if sequence != expected && log.is_buffer_full() {
            let (from, to) = log
                .close_gap()
                .expect("a full buffer holds at least one message");

            closed_gap = Some(MessageGapClosed {
                conversation,
                author: log.author(),
                from,
                to,
                cause: GapCloseCause::BufferFull,
            });

            // The close moved the mark, so this message is judged again against
            // the state it produced. It cannot fill the buffer a second time:
            // the close applied at least the message it was blocking.
            if log.is_out_of_reach(sequence) {
                return Ok(InboundOutcome::new(
                    Self::rejection(conversation, log.author(), sequence),
                    received_since(log, visible_before),
                    closed_gap,
                ));
            }
            expected = SequenceNumber::following(log.high_water_mark())?;
        }

        // R.4, R.6 — at first contact `expected` is FIRST, so genesis applies
        // with no settling delay.
        if sequence == expected {
            log.apply(message);
            log.drain_contiguous();

            return Ok(InboundOutcome::new(
                MessagePlacement::Applied(id),
                received_since(log, visible_before),
                closed_gap,
            ));
        }

        // R.5, R.7
        log.buffer(message, received_at);

        Ok(InboundOutcome::new(
            MessagePlacement::Buffered {
                id,
                awaiting: expected,
            },
            received_since(log, visible_before),
            closed_gap,
        ))
    }

    fn rejection(
        conversation: ConversationId,
        author: PeerId,
        sequence: SequenceNumber,
    ) -> MessagePlacement {
        MessagePlacement::Rejected(MessageRejected {
            conversation,
            claimed_author: author,
            sequence: Some(sequence),
            reason: RejectionReason::ArrivedAfterGapClosed,
        })
    }

    fn applied_mut(&mut self, id: &MessageId) -> Result<&mut Message, ConversationError> {
        if id.conversation() != self.id {
            return Err(ConversationError::WrongConversation);
        }

        self.authors
            .get_mut(&id.author())
            .and_then(|log| log.applied_mut(id.sequence()))
            .ok_or(ConversationError::UnknownMessage)
    }

    fn log_mut(&mut self, author: PeerId) -> &mut AuthorLog {
        self.authors
            .entry(author)
            .or_insert_with(|| AuthorLog::empty(author))
    }
}

/// The messages a log gained past `from`, as events, in the author's order.
fn received_since(log: &AuthorLog, from: usize) -> Vec<MessageReceived> {
    log.messages()[from..]
        .iter()
        .map(|message| MessageReceived {
            id: message.id(),
            claimed_sent_at: message.claimed_sent_at(),
        })
        .collect()
}

/// Typed rejection of a [`Conversation`] operation.
///
/// These are caller mistakes, not network conditions. What the network does —
/// reorder, duplicate, flood, lose — comes back as an [`InboundOutcome`]
/// instead, because none of it is anyone's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationError {
    /// A direct conversation was asked for between the local peer and itself.
    SelfConversation,
    /// An inbound message named the local peer as its author. Locally composed
    /// messages enter through [`Conversation::append_local`]; anything else
    /// carrying this peer's identity is a replay of its own traffic.
    SelfAuthoredInbound,
    /// The author is neither party to this direct conversation.
    AuthorNotInConversation,
    /// The identifier belongs to a different conversation.
    WrongConversation,
    /// No applied message carries that identifier here.
    UnknownMessage,
    /// The author has used every sequence number available in this
    /// conversation (2^64 - 1 messages).
    SequenceExhausted,
    /// The message's delivery state machine has no such move.
    InvalidDeliveryTransition {
        from: DeliveryState,
        to: DeliveryState,
    },
}

impl From<SequenceNumberError> for ConversationError {
    /// A zero sequence number cannot reach here — every path builds one from a
    /// validated [`SequenceNumber`] — so the only remaining cause is
    /// exhaustion.
    fn from(_: SequenceNumberError) -> Self {
        Self::SequenceExhausted
    }
}

impl From<DeliveryStateError> for ConversationError {
    fn from(error: DeliveryStateError) -> Self {
        match error {
            DeliveryStateError::InvalidTransition { from, to } => {
                Self::InvalidDeliveryTransition { from, to }
            }
        }
    }
}

impl fmt::Display for ConversationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfConversation => f.write_str("a direct conversation needs two distinct peers"),
            Self::SelfAuthoredInbound => {
                f.write_str("an inbound message cannot be authored by the local peer")
            }
            Self::AuthorNotInConversation => {
                f.write_str("the author is not a party to this conversation")
            }
            Self::WrongConversation => {
                f.write_str("the message identifier belongs to another conversation")
            }
            Self::UnknownMessage => f.write_str("no such message in this conversation"),
            Self::SequenceExhausted => {
                f.write_str("the author has no sequence number left in this conversation")
            }
            Self::InvalidDeliveryTransition { from, to } => {
                write!(f, "delivery state cannot move from {from} to {to}")
            }
        }
    }
}

impl std::error::Error for ConversationError {}
