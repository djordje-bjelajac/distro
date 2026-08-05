use messaging::domain::events::{GapCloseCause, MessageGapClosed};
use messaging::domain::{DeliveryState, Message, SequenceNumber};
use shared_types::PeerId;

use crate::composition::abandoned_span;
use crate::tui::PeerLabels;

/// One conversation as the pane draws it: the applied messages grouped by
/// author, with every abandoned run marked in place.
///
/// # The shape is the read model's, and it is not an interleaving
///
/// `MessagingQueryPort::history` says so plainly:
///
/// > *Grouped by author in `PeerId` order, and within an author in that
/// > author's send order. There is no order **across** authors, and none is
/// > invented: with no global clock and no consensus there is nothing to derive
/// > one from.*
///
/// So a 1:1 conversation does **not** arrive as a back-and-forth, and this view
/// does not pretend it does. Rendering it as one column ordered by the
/// `claimed_sent_at` field would be inventing a chronology out of two
/// unsynchronised clocks, one of which the remote peer chooses freely — a
/// conversation that reads perfectly and is a lie whenever it matters.
///
/// The pane therefore draws one block per author, headed by that author, each
/// block in that author's own send order. That is exactly what AC8 promises and
/// exactly what the domain can back. It reads less like a chat window than a
/// user expects, and it is honest; a real interleaving needs a domain rule that
/// does not exist, and inventing one in a render path is the last place it
/// should happen.
///
/// # Gaps are placed, not appended
///
/// An abandoned run (AC15) is inserted at the position it occupies in its
/// author's sequence — after the last message below it, before the first
/// message above it — because that is the *only* place it explains anything.
/// A marker at the bottom of the pane would tell a user that something was lost
/// without telling them where the hole is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationView {
    /// One block per author, in the order the read model returned them.
    pub authors: Vec<AuthorRun>,
}

impl ConversationView {
    /// Builds the view from what the query port and the gap ledger hold.
    ///
    /// Both inputs are taken as read: `history` has already applied
    /// invariant 5's ordering and invariant 10's rejection, and every gap is a
    /// domain event recorded verbatim. Nothing here decides what is visible.
    pub fn build(history: &[Message], gaps: &[MessageGapClosed], labels: PeerLabels) -> Self {
        let mut authors: Vec<AuthorRun> = Vec::new();

        for message in history {
            let author = message.author();
            let index = match authors.iter().position(|run| run.author == author) {
                Some(index) => index,
                None => {
                    authors.push(AuthorRun::empty(author, labels));
                    authors.len() - 1
                }
            };

            authors[index].entries.push(Entry::Message {
                sequence: message.sequence(),
                body: message.body().to_string(),
                delivery: message.delivery_state(),
            });
        }

        // An author whose every message was abandoned has a gap and no
        // messages, and must still appear — otherwise the loss is invisible,
        // which is the one thing AC15 forbids.
        for gap in gaps {
            if !authors.iter().any(|run| run.author == gap.author) {
                authors.push(AuthorRun::empty(gap.author, labels));
            }
        }

        for run in &mut authors {
            run.insert_gaps(gaps);
        }

        Self { authors }
    }

    /// Whether there is anything at all to draw.
    pub fn is_empty(&self) -> bool {
        self.authors.iter().all(|run| run.entries.is_empty())
    }
}

/// One author's messages, in that author's send order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorRun {
    pub author: PeerId,
    pub label: String,
    pub is_local: bool,
    pub entries: Vec<Entry>,
}

impl AuthorRun {
    fn empty(author: PeerId, labels: PeerLabels) -> Self {
        Self {
            author,
            label: labels.label(author),
            is_local: labels.is_local(author),
            entries: Vec::new(),
        }
    }

    /// Places every abandoned run of this author at its own position.
    fn insert_gaps(&mut self, gaps: &[MessageGapClosed]) {
        let mut mine: Vec<&MessageGapClosed> = gaps
            .iter()
            .filter(|gap| gap.author == self.author)
            .collect();
        // Highest first, so each insertion index stays valid as earlier gaps go
        // in below it.
        mine.sort_by_key(|gap| std::cmp::Reverse(gap.from.as_u64()));

        for gap in mine {
            let at = self
                .entries
                .iter()
                .position(|entry| entry.sequence().is_some_and(|sequence| sequence > gap.to))
                .unwrap_or(self.entries.len());

            self.entries.insert(
                at,
                Entry::AbandonedRun {
                    from: gap.from,
                    to: gap.to,
                    messages: abandoned_span(gap),
                    cause: gap.cause,
                },
            );
        }
    }
}

/// One thing inside an author's block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A message that was applied and is displayed.
    Message {
        sequence: SequenceNumber,
        body: String,
        delivery: DeliveryState,
    },
    /// A run this peer gave up on and will never display (AC15).
    AbandonedRun {
        from: SequenceNumber,
        to: SequenceNumber,
        messages: u64,
        cause: GapCloseCause,
    },
}

impl Entry {
    const fn sequence(&self) -> Option<SequenceNumber> {
        match self {
            Self::Message { sequence, .. } => Some(*sequence),
            Self::AbandonedRun { .. } => None,
        }
    }

    /// The sentence a user reads for an abandoned run, or `None` for a
    /// message.
    ///
    /// `author` is passed in rather than held because the run already knows it,
    /// and a marker repeating the author inside its own author's block would be
    /// noise everywhere except the one place it is read out of context.
    pub fn abandoned_text(&self, author_label: &str) -> Option<String> {
        let Self::AbandonedRun {
            messages, cause, ..
        } = self
        else {
            return None;
        };

        let plural = if *messages == 1 {
            "message"
        } else {
            "messages"
        };
        let why = match cause {
            GapCloseCause::ToleranceElapsed => "they did not arrive in time",
            GapCloseCause::BufferFull => "too much arrived out of order to hold",
        };

        Some(format!(
            "{messages} {plural} from {author_label} were never received — {why}"
        ))
    }
}

/// The per-message delivery mark (AC11).
///
/// Four states, four marks, and no fifth: AC11 makes silent loss a non-state,
/// so a message is always one of pending, delivered, failed-with-a-reason, or
/// published. A blank would be the fifth.
pub const fn delivery_mark(delivery: DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Pending => "·",
        DeliveryState::Delivered => "✓",
        DeliveryState::Failed(_) => "✗",
        DeliveryState::Published => "→",
    }
}

/// The delivery mark plus, for a failure, the reason a user can act on.
pub fn delivery_text(delivery: DeliveryState) -> String {
    match delivery {
        DeliveryState::Failed(reason) => format!("✗ {reason}"),
        other => format!("{} {other}", delivery_mark(other)),
    }
}
