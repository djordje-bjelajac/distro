use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The one clock behind both contexts' `ClockPort` (D11, S5).
///
/// # Why the origin has to be the UNIX epoch
///
/// `membership`'s `Millis` documents its origin as unspecified, and for
/// presence that is right: only the *difference* between two of this peer's own
/// readings matters. One value breaks that rule by design —
/// [`JoinTicket`](membership::domain::JoinTicket)'s expiry. A ticket is minted
/// on one machine and validated on another against *that* machine's clock
/// (flagged by OP-10 on `JoinTicketCodec::encode`, and left to this operation
/// to decide). If the two peers' timelines have different origins the
/// comparison is meaningless: a ticket would look expired the moment it was
/// issued, or valid forever. So the origin is UNIX-epoch milliseconds, which
/// two machines already agree on to within their NTP error.
///
/// # Why the advance is not the wall clock
///
/// `membership`'s port states the other half of the contract: successive
/// readings never decrease. A wall clock breaks that — NTP steps it, a user
/// sets it, a laptop resumes with a corrected one — and a backwards jump would
/// make every age negative and, saturated to zero, would re-arm the whole
/// roster as freshly seen.
///
/// So this clock samples the wall clock **once**, at construction, and advances
/// from there on a monotonic [`Instant`]. Both properties hold at once:
///
/// * shared origin, so a ticket minted elsewhere means something here;
/// * monotonic advance, so no NTP step can move a reading backwards.
///
/// The cost is that a correction arriving after startup is not applied — this
/// peer's absolute reading drifts from true UNIX time by whatever its clock
/// drifts by, plus whatever the anchor was wrong by. For a ticket lifetime of
/// 24 hours (`JoinTicket::DEFAULT_LIFETIME`) that is not a quantity that
/// matters, and a monotonic presence window is worth far more than an exact
/// wall reading.
///
/// # One clock, two ports, on purpose
///
/// `membership` and `messaging` each declare their own `ClockPort` because
/// neither may import the other and `shared_types` hosts no port traits (canvas
/// §2.4, §4). Both are implemented here over one anchor, so a roster ageing
/// presence and a conversation ageing a gap can never disagree about what time
/// it is.
#[derive(Debug)]
pub struct SystemClock {
    /// UNIX-epoch milliseconds sampled once, at construction.
    anchor_millis: u64,
    /// The monotonic source every later reading advances on.
    anchored_at: Instant,
}

impl SystemClock {
    /// Anchors the timeline to this machine's current wall-clock reading.
    pub fn now() -> Self {
        Self::anchored_to(unix_epoch_millis())
    }

    /// Anchors the timeline to an explicit UNIX-epoch reading.
    ///
    /// Exposed so a test can pin the origin without touching the machine's
    /// clock; production always uses [`now`](Self::now).
    pub fn anchored_to(anchor_millis: u64) -> Self {
        Self {
            anchor_millis,
            anchored_at: Instant::now(),
        }
    }

    /// The current reading, in milliseconds since the UNIX epoch.
    pub fn epoch_millis(&self) -> u64 {
        let elapsed = u64::try_from(self.anchored_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        self.anchor_millis.saturating_add(elapsed)
    }
}

impl membership::ports::ClockPort for SystemClock {
    fn now(&self) -> membership::domain::Millis {
        membership::domain::Millis::from_millis(self.epoch_millis())
    }
}

impl messaging::ports::ClockPort for SystemClock {
    fn now(&self) -> messaging::domain::Millis {
        messaging::domain::Millis::from_millis(self.epoch_millis())
    }
}

/// This machine's wall clock, in milliseconds since the UNIX epoch.
///
/// A reading before the epoch — only reachable on a machine whose clock was
/// never set — saturates to zero rather than failing the launch: an identity
/// still loads, a LAN join still works, and only ticket expiry is affected,
/// which is precisely what a machine with no clock cannot get right anyway.
fn unix_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}
