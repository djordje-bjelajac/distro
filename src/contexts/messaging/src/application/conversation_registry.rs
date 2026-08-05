use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use shared_types::PeerId;

use crate::domain::{Conversation, ConversationId};
use crate::ports::{MessagingCommandError, SequenceCounterPort};

/// Every [`Conversation`] this process is holding: the broadcast channel and
/// one per remote peer it has spoken with or heard from.
///
/// # Why the application layer holds them
///
/// History is in-memory only in v1 (D7) and the canvas declares no port that
/// would load an aggregate back, so the conversations live for as long as the
/// process does — like `identity`'s `LocalIdentityState` and `membership`'s
/// `MembershipState`. `MessageLogPort` is a *mirror* of what has been applied,
/// not a repository these are read from: a durable adapter behind it is a later
/// drop-in that touches no domain code.
///
/// # Rehydration is not optional
///
/// A conversation is only ever created through
/// [`Conversation::rehydrate`], with the mark this peer's outbound counter has
/// reached (D12, AC16). Doing it here rather than in each handler is what makes
/// it impossible to skip: a peer that resumed at sequence 1 after a restart
/// would have every message it sent classified a duplicate by listeners still
/// holding its old high-water mark — going permanently mute while appearing, to
/// itself, to work.
///
/// # Interior mutability, and one rule about it
///
/// A composition root drives this context from more than one task — a network
/// pump taking in envelopes, a UI composing messages, a ticker sweeping gaps —
/// so the cell must be `Sync`; `Mutex` is the std answer and this crate takes no
/// async runtime dependency. A poisoned lock is recovered rather than
/// propagated, so one failed assertion cannot turn every later read into a
/// panic.
///
/// **No caller may hold the lock across a call into a port.** A port may
/// legitimately call back into this context — a transport adapter asking what a
/// conversation holds while a send is in flight is the obvious case — and a lock
/// held across that boundary turns a read into a deadlock. Every method below
/// therefore runs a closure of pure domain work under the lock and releases it
/// before anything else happens; [`modify`](Self::modify) releases it
/// specifically to ask the counter.
pub struct ConversationRegistry {
    local: PeerId,
    counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    conversations: Mutex<BTreeMap<ConversationId, Conversation>>,
}

impl ConversationRegistry {
    /// An empty registry belonging to `local`, drawing outbound sequence marks
    /// from `counter`.
    pub fn for_local_peer(
        local: PeerId,
        counter: Arc<dyn SequenceCounterPort + Send + Sync>,
    ) -> Self {
        Self {
            local,
            counter,
            conversations: Mutex::new(BTreeMap::new()),
        }
    }

    /// The peer this registry's conversations belong to.
    pub const fn local_peer(&self) -> PeerId {
        self.local
    }

    /// Runs pure domain work against one conversation, opening it — rehydrated
    /// from the counter — if this process has not touched it yet.
    ///
    /// `change` must not call a port: it runs under the lock. Handlers
    /// therefore return the events a transition produced and publish them
    /// afterwards.
    pub(crate) fn modify<R>(
        &self,
        id: ConversationId,
        change: impl FnOnce(&mut Conversation) -> R,
    ) -> Result<R, MessagingCommandError> {
        // Fast path: already open, so no port is called and the lock covers
        // pure work only.
        {
            let mut open = self.lock();
            if let Some(conversation) = open.get_mut(&id) {
                return Ok(change(conversation));
            }
        }

        // The lock is released *before* the counter is asked, because the
        // counter is a port and a port may call back into this context.
        let last_issued = self.counter.last_issued(id)?;
        let rehydrated = Conversation::rehydrate(id, self.local, last_issued)?;

        let mut open = self.lock();
        // Another task may have opened it while the counter was answering. Its
        // instance wins; both were rehydrated from the same mark, so they are
        // equivalent, and discarding a duplicate is cheaper than holding the
        // lock across the port call.
        let conversation = open.entry(id).or_insert(rehydrated);
        Ok(change(conversation))
    }

    /// Runs pure domain work against one conversation **only if it is already
    /// open**; `None` when it is not.
    ///
    /// The variant for news that names a conversation this peer may never have
    /// had — a delivery acknowledgement for a message it does not hold, a
    /// disconnect from a peer it never messaged. Opening one in response would
    /// let stray traffic populate the conversation list.
    pub(crate) fn modify_open<R>(
        &self,
        id: ConversationId,
        change: impl FnOnce(&mut Conversation) -> R,
    ) -> Option<R> {
        self.lock().get_mut(&id).map(change)
    }

    /// Reads one conversation; `None` when it is not open.
    ///
    /// Reading never opens one. That is what makes the query side genuinely
    /// read-only: rendering an empty conversation must not bring it into
    /// existence, or a redraw would change what `conversations` reports.
    pub(crate) fn read<R>(
        &self,
        id: ConversationId,
        view: impl FnOnce(&Conversation) -> R,
    ) -> Option<R> {
        self.lock().get(&id).map(view)
    }

    /// Runs pure domain work against every open conversation, in
    /// [`ConversationId`] order so a sweep's effects are deterministic (AC13).
    ///
    /// `visit` must not call a port: it runs under the lock, once per
    /// conversation.
    pub(crate) fn sweep<R>(&self, mut visit: impl FnMut(&mut Conversation) -> R) -> Vec<R> {
        self.lock().values_mut().map(&mut visit).collect()
    }

    /// Every conversation currently open, in [`ConversationId`] order.
    ///
    /// Test-only. Production reads the conversation *listing* from
    /// `MessageLogPort`, because a conversation nobody has said anything in is
    /// not one a user should be shown; this is how a test asks the sharper
    /// question of whether one was opened at all.
    #[cfg(test)]
    pub(crate) fn open_conversations(&self) -> Vec<ConversationId> {
        self.lock().keys().copied().collect()
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<ConversationId, Conversation>> {
        self.conversations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
