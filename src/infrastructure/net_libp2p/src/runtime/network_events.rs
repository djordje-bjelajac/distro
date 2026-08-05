use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::swarm::NetworkEvent;

/// The stream of things the network reports, drained synchronously.
///
/// # Why the root drains rather than the driver pushing
///
/// The driver could hold `InboundSessionPort` and `InboundEnvelopePort` and
/// call them itself. It deliberately does not (see [`NetworkEvent`]): that
/// would have an infrastructure crate decide which thread two contexts'
/// aggregates are mutated on. Draining here leaves the composition root in
/// charge of its own concurrency, which is the only place that decision belongs.
///
/// # Bounded, and overflow is counted
///
/// The queue behind this holds
/// [`ResourceLimits::event_queue_capacity`](crate::limits::ResourceLimits::event_queue_capacity)
/// events. A root that stops draining does not grow this process without
/// limit; it loses events, and every loss is counted in
/// [`CodecDiagnostics::dropped_events`](crate::codec::CodecDiagnostics::dropped_events).
/// Bounded-and-counted beats unbounded-and-silent: the first is a number on a
/// diagnostics pane, the second is a memory leak nobody notices until the
/// process dies.
///
/// # Threading
///
/// `Send + Sync`, so a root can hold it behind an `Arc` and drain from a
/// dedicated thread. The `Mutex` serialises concurrent drainers; it is never
/// held across anything that blocks except the timeout in
/// [`next_timeout`](Self::next_timeout), which is the caller's own choice.
pub struct NetworkEvents {
    receiver: Mutex<Receiver<NetworkEvent>>,
}

impl NetworkEvents {
    pub(crate) const fn new(receiver: Receiver<NetworkEvent>) -> Self {
        Self {
            receiver: Mutex::new(receiver),
        }
    }

    /// The next event, or `None` when none is waiting. Never blocks.
    pub fn try_next(&self) -> Option<NetworkEvent> {
        self.lock().try_recv().ok()
    }

    /// The next event, waiting up to `timeout`.
    ///
    /// `None` means the wait elapsed or the driver has stopped — a caller that
    /// needs to tell those apart should check the runtime, but neither is a
    /// reason to keep waiting.
    pub fn next_timeout(&self, timeout: Duration) -> Option<NetworkEvent> {
        self.lock().recv_timeout(timeout).ok()
    }

    /// Every event waiting right now, oldest first.
    pub fn drain(&self) -> Vec<NetworkEvent> {
        let receiver = self.lock();
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        events
    }

    /// A poisoned lock means a previous drainer panicked mid-drain. The
    /// receiver itself has no invariant that panic could have broken, so
    /// recovering is correct and refusing to would take the whole network down
    /// for a bug elsewhere.
    fn lock(&self) -> std::sync::MutexGuard<'_, Receiver<NetworkEvent>> {
        self.receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl std::fmt::Debug for NetworkEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NetworkEvents")
    }
}
