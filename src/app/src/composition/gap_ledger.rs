use std::collections::VecDeque;
use std::sync::Mutex;

use messaging::domain::ConversationId;
use messaging::domain::events::MessageGapClosed;

/// Every abandoned run this peer has decided it will never display, kept so the
/// conversation pane can say so (AC15).
///
/// # Why the root has to keep them at all
///
/// `MessageGapClosed` is an *event*, and `MessagingQueryPort` deliberately does
/// not carry it: the read model is the applied run and nothing else. So a
/// conversation rendered from `history` alone shows a jump from sequence 4 to
/// sequence 9 with nothing to explain it, which is exactly the silent loss AC15
/// exists to forbid. The events reach here through `messaging`'s
/// `EventPublisherPort`, which is the only place both close causes appear —
/// the clock-driven sweep *and* the buffer-full close that happens inside an
/// ordinary `accept_envelope`.
///
/// # This invents nothing
///
/// A ledger entry is a domain event, stored verbatim and rendered verbatim. The
/// root does not decide what was lost, when to give up, or how to display an
/// author's order — those are rule R's, and they already happened before the
/// event was published.
///
/// # Bounded
///
/// Held newest-last with the oldest discarded past the cap: a peer being
/// flooded by an author whose messages never arrive would otherwise accumulate
/// one entry per gap forever. The count of everything ever abandoned lives in
/// `Diagnostics` and is never discarded, so the pane can still say "and 400
/// more" honestly.
#[derive(Debug)]
pub struct GapLedger {
    entries: Mutex<VecDeque<MessageGapClosed>>,
    capacity: usize,
}

impl GapLedger {
    /// Abandoned runs remembered for display.
    pub const DEFAULT_CAPACITY: usize = 256;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
        }
    }

    /// Remembers one abandoned run.
    pub fn record(&self, event: MessageGapClosed) {
        let mut entries = self.lock();

        while entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(event);
    }

    /// The abandoned runs in one conversation, oldest first.
    pub fn of(&self, conversation: ConversationId) -> Vec<MessageGapClosed> {
        self.lock()
            .iter()
            .filter(|event| event.conversation == conversation)
            .copied()
            .collect()
    }

    /// Every abandoned run currently remembered, oldest first.
    pub fn all(&self) -> Vec<MessageGapClosed> {
        self.lock().iter().copied().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<MessageGapClosed>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for GapLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// How many of an author's messages a closed gap wrote off.
///
/// The range is inclusive, so a one-message gap has `from == to` and a span of
/// one. Saturating rather than wrapping: a malformed pair can only ever
/// under-report, never produce an enormous count in a diagnostic.
pub const fn abandoned_span(event: &MessageGapClosed) -> u64 {
    event
        .to
        .as_u64()
        .saturating_sub(event.from.as_u64())
        .saturating_add(1)
}
