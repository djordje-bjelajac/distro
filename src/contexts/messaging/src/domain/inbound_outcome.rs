use crate::domain::MessagePlacement;
use crate::domain::events::{MessageGapClosed, MessageReceived, MessagingEvent};

/// Everything a [`Conversation`](crate::domain::Conversation) did while taking
/// in one inbound message.
///
/// Three facts, because they are independent and the application needs all
/// three:
///
/// - [`placement`](Self::placement) — what happened to *this* message.
/// - [`applied`](Self::applied) — every message that became visible, in the
///   author's order. Usually the arrival plus whatever it unblocked, but a
///   message can be refused and still release a run: abandoning a gap to make
///   room is one call in which both happen.
/// - [`closed_gap`](Self::closed_gap) — whether a range was given up on to get
///   there (AC15). Reaching the per-author buffer cap is that trigger; the
///   other is time, and it runs through
///   [`Conversation::close_aged_gaps`](crate::domain::Conversation::close_aged_gaps).
///
/// Buffering produces no event: nothing has happened to the conversation yet.
/// The message is held, invisible to every read view, until the run leading to
/// it completes — at which point it appears in a later `applied` — or until its
/// gap is abandoned, at which point it appears together with a `closed_gap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundOutcome {
    placement: MessagePlacement,
    applied: Vec<MessageReceived>,
    closed_gap: Option<MessageGapClosed>,
}

impl InboundOutcome {
    pub(super) const fn new(
        placement: MessagePlacement,
        applied: Vec<MessageReceived>,
        closed_gap: Option<MessageGapClosed>,
    ) -> Self {
        Self {
            placement,
            applied,
            closed_gap,
        }
    }

    /// What happened to the message that was handed in.
    pub const fn placement(&self) -> &MessagePlacement {
        &self.placement
    }

    /// The messages that became visible, in the author's send order — empty
    /// when nothing did.
    pub fn applied(&self) -> &[MessageReceived] {
        &self.applied
    }

    /// The range this call gave up on, if it gave up on one.
    pub const fn closed_gap(&self) -> Option<MessageGapClosed> {
        self.closed_gap
    }

    pub const fn is_applied(&self) -> bool {
        matches!(self.placement, MessagePlacement::Applied(_))
    }

    pub const fn is_buffered(&self) -> bool {
        matches!(self.placement, MessagePlacement::Buffered { .. })
    }

    pub const fn is_duplicate(&self) -> bool {
        matches!(self.placement, MessagePlacement::DuplicateIgnored(_))
    }

    pub const fn is_rejected(&self) -> bool {
        matches!(self.placement, MessagePlacement::Rejected(_))
    }

    /// The events the application must publish for this outcome, in order.
    ///
    /// The domain holds no publisher port, so it returns what happened and the
    /// application delivers it. An abandoned gap comes first: it is what
    /// explains the jump in the messages that follow it, and a consumer given
    /// the other order would have to reason backwards from a hole it had
    /// already rendered.
    pub fn into_events(self) -> Vec<MessagingEvent> {
        let mut events: Vec<MessagingEvent> = self
            .closed_gap
            .map(MessagingEvent::from)
            .into_iter()
            .collect();

        events.extend(self.applied.into_iter().map(MessagingEvent::from));

        match self.placement {
            MessagePlacement::Applied(_) | MessagePlacement::Buffered { .. } => {}
            MessagePlacement::DuplicateIgnored(event) => events.push(event.into()),
            MessagePlacement::Rejected(event) => events.push(event.into()),
        }

        events
    }
}
