use membership::domain::LivenessWindows;
use messaging::domain::Conversation;

/// When the engine's four clock-driven duties are next due.
///
/// # Why any of this is the root's job
///
/// Both contexts document it in the same words: *nothing here starts a task, a
/// thread, or a timer.* That is what keeps every one of their tests free of
/// real time (AC13) — and it makes driving them a stated obligation of the
/// composition root rather than a hidden thread nobody can see. Two of the four
/// duties below were flagged as missing by the operations that defined them:
///
/// * **Presence** — `InboundSessionPort::expire_presence`. Without a caller,
///   AC5 never fires: no peer is ever observed to depart, and every roster row
///   stays as it was the last time anything arrived.
/// * **Gap sweep** — `InboundEnvelopePort::close_aged_gaps`. Without a caller,
///   a gap closes only when a per-author buffer fills, which on a quiet
///   conversation may be never — so an author whose message was lost simply
///   stops being heard, silently, forever (AC10, AC15).
///
/// and two more the root needs for itself:
///
/// * **Heartbeat** — OP-10 emits no liveness probe by design, so the
///   application does. Without one, a peer that is merely quiet becomes
///   `Offline` in every other roster.
/// * **Trust refresh** — the block list the author policy answers from is
///   loaded ahead of time (see `TrustDirectory`), so something has to reload
///   it.
///
/// # Why the intervals are what they are
///
/// * **Presence and heartbeat share
///   [`LivenessWindows::HEARTBEAT_INTERVAL`]** (10 s) — the cadence the
///   presence windows are *derived from*. A peer is `Online` for three missed
///   heartbeats and `Offline` after six, so emitting and evaluating on that
///   same beat is what makes those windows mean what they say. Deriving the
///   interval from the domain constant rather than repeating `10_000` is what
///   keeps them from drifting apart.
/// * **The gap sweep runs four times per tolerance window.** The window is
///   [`Conversation::GAP_TOLERANCE`] (2 s), and a sweep is the only thing that
///   closes a gap on a quiet conversation. Sweeping once per window would let a
///   gap live up to twice its tolerance; four times bounds the overshoot at a
///   quarter of it, for a pass over conversations that is cheap and idempotent.
/// * **Trust is reloaded every second.** The file holds the peers one human has
///   verified or blocked. A block a user just made is applied immediately by
///   the command path, so this only has to catch a file changed by something
///   else — a second instance on the same profile, an editor — and one second
///   is imperceptible for that.
///
/// # Ticks do not accumulate
///
/// Each due time is recomputed from `now`, not advanced by one interval. A
/// process suspended for an hour therefore performs each duty **once** on
/// resume rather than three hundred and sixty times — and since every one of
/// them is idempotent, once is exactly right. A catch-up backlog would spend
/// the first seconds after a laptop wakes re-sweeping the same conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickSchedule {
    presence_interval: u64,
    gap_interval: u64,
    trust_interval: u64,

    next_presence: u64,
    next_gap: u64,
    next_trust: u64,
}

impl TickSchedule {
    /// How often presence is swept and a heartbeat emitted.
    pub const PRESENCE_INTERVAL_MILLIS: u64 = LivenessWindows::HEARTBEAT_INTERVAL.as_millis();

    /// Sweeps per gap-tolerance window.
    const GAP_SWEEPS_PER_WINDOW: u64 = 4;

    /// How often aged gaps are closed.
    pub const GAP_INTERVAL_MILLIS: u64 =
        Conversation::GAP_TOLERANCE.as_millis() / Self::GAP_SWEEPS_PER_WINDOW;

    /// How often the block list is reloaded.
    pub const TRUST_INTERVAL_MILLIS: u64 = 1_000;

    /// A schedule whose first tick of each kind is due immediately at `now`.
    ///
    /// Immediately on purpose: the first presence sweep costs nothing on an
    /// empty roster, and the first heartbeat is what tells the network this
    /// peer exists.
    pub const fn starting_at(now: u64) -> Self {
        Self {
            presence_interval: Self::PRESENCE_INTERVAL_MILLIS,
            gap_interval: Self::GAP_INTERVAL_MILLIS,
            trust_interval: Self::TRUST_INTERVAL_MILLIS,
            next_presence: now,
            next_gap: now,
            next_trust: now,
        }
    }

    /// A schedule with explicit intervals, so a test can drive one without
    /// waiting ten seconds. Zero is refused into one millisecond: an interval
    /// of zero is a busy loop, never what a caller meant.
    pub const fn with_intervals(now: u64, presence: u64, gap: u64, trust: u64) -> Self {
        Self {
            presence_interval: if presence == 0 { 1 } else { presence },
            gap_interval: if gap == 0 { 1 } else { gap },
            trust_interval: if trust == 0 { 1 } else { trust },
            next_presence: now,
            next_gap: now,
            next_trust: now,
        }
    }

    /// Which duties are due at `now`, marking each one performed.
    pub const fn due(&mut self, now: u64) -> DueTicks {
        let presence = now >= self.next_presence;
        if presence {
            self.next_presence = now.saturating_add(self.presence_interval);
        }

        let gaps = now >= self.next_gap;
        if gaps {
            self.next_gap = now.saturating_add(self.gap_interval);
        }

        let trust = now >= self.next_trust;
        if trust {
            self.next_trust = now.saturating_add(self.trust_interval);
        }

        DueTicks {
            presence,
            gaps,
            trust,
        }
    }
}

/// What the engine must do on this pass.
///
/// `presence` covers both halves of the same beat — sweep this peer's view of
/// others, and give others evidence of this one — because they are the same
/// cadence by construction and separating them would invite two constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DueTicks {
    /// Expire presence (AC5) and emit a heartbeat.
    pub presence: bool,
    /// Close gaps that have waited past the tolerance window (AC15).
    pub gaps: bool,
    /// Reload the block list the author policy answers from (invariant 11).
    pub trust: bool,
}

impl DueTicks {
    /// Whether anything at all is due.
    pub const fn any(&self) -> bool {
        self.presence || self.gaps || self.trust
    }
}
