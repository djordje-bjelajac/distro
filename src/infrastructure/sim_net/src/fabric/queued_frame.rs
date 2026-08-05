use shared_types::PeerId;

use crate::fabric::SimFrame;

/// One frame waiting in the fabric, with everything needed to order it
/// deterministically.
///
/// # The two-part key is the determinism guarantee
///
/// Delivery order is `(due_at, id)` and nothing else. `due_at` is the virtual
/// instant the frame becomes deliverable — link delay plus whatever the script
/// said — and `id` is a strictly increasing enqueue counter that breaks every
/// tie the same way in every run. Without the counter, two frames due at the
/// same instant would be ordered by whatever the queue's internal layout
/// happened to be, which is precisely the kind of dependence AC13 rules out.
///
/// The frame is never sorted by peer, by hash, or by any address, so adding a
/// peer to a scenario cannot reorder traffic between the peers already in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFrame {
    /// Enqueue order: the tiebreaker, and the whole reason two runs agree.
    pub id: u64,
    /// The virtual instant this becomes deliverable, in milliseconds.
    pub due_at: u64,
    /// The peer that sent it.
    pub from: PeerId,
    /// The peer it is for.
    pub to: PeerId,
    /// What is in flight.
    pub frame: SimFrame,
}

impl QueuedFrame {
    /// The ordering key: earliest due instant first, then enqueue order.
    pub const fn key(&self) -> (u64, u64) {
        (self.due_at, self.id)
    }
}
