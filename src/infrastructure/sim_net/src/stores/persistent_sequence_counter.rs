use std::collections::BTreeMap;
use std::sync::Mutex;

use messaging::domain::{ConversationId, SequenceNumber};
use messaging::ports::{SequenceCounterError, SequenceCounterPort};

use crate::stores::guard;

/// The local peer's outbound sequence counter, with the keypair's lifetime
/// (D12, AC16).
///
/// # The restart this type exists for
///
/// With in-memory-only history (D7), a restarted peer used to resume at
/// [`SequenceNumber::FIRST`] while every peer still online held its high-water
/// mark at N — so each message it sent was, correctly by the receiver's rules,
/// classified a duplicate and ignored. The peer went permanently mute while
/// appearing, to itself, to work.
///
/// The harness models the fix exactly: this store is held *outside* a peer's
/// contexts, beside its keypair, and handed unchanged to every rebuild while
/// the conversation log is replaced with an empty one. That asymmetry — counter
/// survives, history does not — is the precise condition AC16 tests, and it is
/// a property of how the harness assembles a peer rather than something a
/// scenario has to remember to arrange.
///
/// # Recorded before it is returned
///
/// The port requires that an advance be recorded before `issue_next` returns: a
/// number handed out but not persisted is a number that will be re-issued after
/// a crash, which is the very failure this port prevents. Here the map *is* the
/// store, so the write and the return are one operation — and the injectable
/// [`NotPersisted`](SequenceCounterError::NotPersisted) fault leaves the map
/// untouched, so a scenario can prove the caller sends nothing.
#[derive(Debug, Default)]
pub struct PersistentSequenceCounter {
    state: Mutex<CounterState>,
}

#[derive(Debug, Default)]
struct CounterState {
    marks: BTreeMap<ConversationId, SequenceNumber>,
    fault: Option<SequenceCounterError>,
}

impl PersistentSequenceCounter {
    /// A counter that has never issued a number: a brand-new identity.
    pub fn fresh() -> Self {
        Self::default()
    }

    /// A counter that has already reached `mark` in `conversation`, as if a
    /// previous process had issued that many messages.
    pub fn resuming_at(conversation: ConversationId, mark: SequenceNumber) -> Self {
        let counter = Self::default();
        guard(&counter.state).marks.insert(conversation, mark);
        counter
    }

    /// The highest number issued in `conversation`, without going through the
    /// port.
    pub fn mark(&self, conversation: ConversationId) -> Option<SequenceNumber> {
        guard(&self.state).marks.get(&conversation).copied()
    }

    /// Every conversation this peer has spoken in, in `ConversationId` order.
    pub fn conversations(&self) -> Vec<ConversationId> {
        guard(&self.state).marks.keys().copied().collect()
    }

    /// Makes every operation fail with `error`, until [`repair`](Self::repair).
    pub fn fail_with(&self, error: SequenceCounterError) {
        guard(&self.state).fault = Some(error);
    }

    /// Clears any injected fault.
    pub fn repair(&self) {
        guard(&self.state).fault = None;
    }
}

impl SequenceCounterPort for PersistentSequenceCounter {
    fn issue_next(
        &self,
        conversation: ConversationId,
    ) -> Result<SequenceNumber, SequenceCounterError> {
        let mut state = guard(&self.state);

        if let Some(error) = state.fault {
            return Err(error);
        }

        let next = SequenceNumber::following(state.marks.get(&conversation).copied())
            .map_err(|_| SequenceCounterError::Exhausted)?;

        // Recorded before it is returned — the port's contract, and the whole
        // reason the counter is a port rather than a field.
        state.marks.insert(conversation, next);
        Ok(next)
    }

    fn last_issued(
        &self,
        conversation: ConversationId,
    ) -> Result<Option<SequenceNumber>, SequenceCounterError> {
        let state = guard(&self.state);

        match state.fault {
            Some(error) => Err(error),
            None => Ok(state.marks.get(&conversation).copied()),
        }
    }
}
