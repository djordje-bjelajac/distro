use std::sync::atomic::{AtomicU64, Ordering};

/// One instant, shared by every context of every simulated peer, that moves
/// only when a scenario says so (D11, S5, AC13).
///
/// # Why one object implements two `ClockPort`s
///
/// `membership` and `messaging` each declare their own `ClockPort` returning
/// their own crate-local `Millis` — they must, because `shared_types` hosts no
/// port traits and contexts never import each other (canvas §2.4, §4). Two
/// traits, however, must not become two *clocks*: a peer whose roster ages
/// presence against one timeline while its conversations age gaps against
/// another would produce results no scenario could reason about, and the
/// disagreement would be invisible until a test failed for the wrong reason.
///
/// So this is a single instant with two projections. The two contexts are
/// wired to the same `Arc<VirtualClock>` at assembly, and it is not possible to
/// advance one without advancing the other.
///
/// # Nothing here reads real time
///
/// There is no `std::time` in this crate. The clock starts where the scenario
/// puts it and moves only through [`advance`](Self::advance) or
/// [`advance_to`](Self::advance_to) — which is what makes "the clock never
/// advances on its own" a testable property rather than a hope.
///
/// # Monotonicity
///
/// Both port contracts require that successive readings never go backwards, so
/// [`advance_to`](Self::advance_to) refuses to move the clock into the past
/// rather than silently rewinding it.
#[derive(Debug)]
pub struct VirtualClock {
    millis: AtomicU64,
}

impl VirtualClock {
    /// Where a clock starts unless a scenario says otherwise.
    ///
    /// Far from zero on purpose: an instant this peer read is then obviously
    /// distinguishable from a default-constructed `Millis::ZERO` and from an
    /// instant a remote peer invented, so a test asserting on a timestamp
    /// cannot pass by accident.
    pub const EPOCH_MILLIS: u64 = 1_000_000;

    /// A clock stopped at [`EPOCH_MILLIS`](Self::EPOCH_MILLIS).
    pub const fn new() -> Self {
        Self::starting_at(Self::EPOCH_MILLIS)
    }

    /// A clock stopped at `millis`.
    pub const fn starting_at(millis: u64) -> Self {
        Self {
            millis: AtomicU64::new(millis),
        }
    }

    /// The current instant, in milliseconds since this clock's origin.
    pub fn now_millis(&self) -> u64 {
        self.millis.load(Ordering::SeqCst)
    }

    /// Moves the clock forward by `millis` and reports the new instant.
    ///
    /// Saturating rather than wrapping: a scenario that jumps to the end of
    /// time stops there instead of reappearing at the beginning, where every
    /// age would flip sign at once.
    pub fn advance(&self, millis: u64) -> u64 {
        let now = self.now_millis().saturating_add(millis);
        self.millis.store(now, Ordering::SeqCst);
        now
    }

    /// Moves the clock forward to `millis`, or leaves it where it is when that
    /// instant has already passed.
    ///
    /// Never moves backwards: both `ClockPort` contracts state that successive
    /// readings never shrink, and a rewound clock would make every derived age
    /// meaningless rather than merely wrong.
    pub fn advance_to(&self, millis: u64) -> u64 {
        let now = self.now_millis().max(millis);
        self.millis.store(now, Ordering::SeqCst);
        now
    }

    /// The current instant as `membership` names it.
    pub fn membership_now(&self) -> membership::domain::Millis {
        membership::domain::Millis::from_millis(self.now_millis())
    }

    /// The current instant as `messaging` names it.
    pub fn messaging_now(&self) -> messaging::domain::Millis {
        messaging::domain::Millis::from_millis(self.now_millis())
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl membership::ports::ClockPort for VirtualClock {
    fn now(&self) -> membership::domain::Millis {
        self.membership_now()
    }
}

impl messaging::ports::ClockPort for VirtualClock {
    fn now(&self) -> messaging::domain::Millis {
        self.messaging_now()
    }
}
