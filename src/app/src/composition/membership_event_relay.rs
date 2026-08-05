use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use membership::domain::events::MembershipEvent;
use membership::ports::{EventPublisherError, EventPublisherPort};

/// `membership`'s `EventPublisherPort`: a bounded queue the root's engine
/// drains.
///
/// # Why a queue rather than a direct call into `messaging`
///
/// This publisher is invoked from *inside* `membership`'s command handlers —
/// while a join is walking the bootstrap ladder, while a session is being
/// recorded — and those calls happen on whatever thread issued the command: the
/// engine, or a join running on its own thread so a multi-second ladder does
/// not stall event draining.
///
/// If this type called `messaging`'s `PeerLifecyclePort` directly, the thread a
/// conversation is mutated on would be decided by whoever happened to issue a
/// membership command. Queueing instead keeps every `messaging` mutation on the
/// engine thread, which is the same reason `infra-net-libp2p` emits
/// `NetworkEvent` values rather than calling inbound ports itself (see
/// `NetworkEvent`'s own documentation).
///
/// # Bounded, and overflow is counted
///
/// The same trade `NetworkEvents` makes, for the same reason: a root that
/// stopped draining loses events and the loss is a number a diagnostics pane
/// can show, rather than a queue that grows until the process dies. The
/// **oldest** events are dropped, not the newest — a `PeerConnected` from a
/// minute ago that nothing consumed is stale, while the disconnect that just
/// arrived is the one a pending direct message is waiting on (D10).
///
/// # Order is preserved
///
/// The port requires it: a `PeerDisconnected` that overtook its `PeerConnected`
/// would leave `messaging` believing a dead peer is live.
#[derive(Debug)]
pub struct MembershipEventRelay {
    queue: Mutex<VecDeque<MembershipEvent>>,
    capacity: usize,
    dropped: AtomicU64,
}

impl MembershipEventRelay {
    /// Events held before the oldest is dropped.
    ///
    /// A join publishes one event per discovered peer plus one per session, so
    /// a burst is the size of a LAN. 1024 absorbs that with room to spare while
    /// bounding what a flood of announcements can make this process hold (S6).
    pub const DEFAULT_CAPACITY: usize = 1024;

    /// A relay holding [`DEFAULT_CAPACITY`](Self::DEFAULT_CAPACITY) events.
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// A relay holding `capacity` events; a capacity of zero holds one, since
    /// a queue that drops everything is never what a caller meant.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            dropped: AtomicU64::new(0),
        }
    }

    /// Every event waiting right now, oldest first.
    pub fn drain(&self) -> Vec<MembershipEvent> {
        self.lock().drain(..).collect()
    }

    /// How many events were dropped because nothing drained in time.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// A poisoned lock means a previous holder panicked mid-drain. A queue has
    /// no invariant a panic could have broken, so recovering is correct.
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<MembershipEvent>> {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for MembershipEventRelay {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPublisherPort for MembershipEventRelay {
    fn publish(&self, event: MembershipEvent) -> Result<(), EventPublisherError> {
        let mut queue = self.lock();

        while queue.len() >= self.capacity {
            queue.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        queue.push_back(event);

        Ok(())
    }
}
