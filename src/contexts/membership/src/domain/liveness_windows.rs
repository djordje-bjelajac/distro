use std::fmt;

use crate::domain::DurationMillis;

/// The two evidence-age thresholds that [`Presence`](crate::domain::Presence)
/// is derived against (canvas §2.2, invariant 7).
///
/// # Why the values are what they are
///
/// The canvas leaves the liveness window as an engineering default (§9), so it
/// is pinned here with its reasoning rather than scattered as magic numbers.
/// Everything is anchored to [`HEARTBEAT_INTERVAL`](Self::HEARTBEAT_INTERVAL),
/// the cadence at which a peer is expected to produce evidence of life:
///
/// * **Online — 3 heartbeats.** A single lost heartbeat, a GC pause, or normal
///   internet jitter must not make a healthy peer flicker in the roster pane.
///   Two consecutive misses are needed before the display changes at all.
/// * **Offline — 6 heartbeats.** AC5 requires that peers observe a departure
///   "within the liveness window"; one minute of total silence is a decision
///   that a peer is gone, not a guess. The three-heartbeat `Stale` band in
///   between exists precisely so the UI can show uncertainty instead of
///   asserting something it cannot know (invariant 7: no peer asserts another's
///   presence as fact).
///
/// The interval is an *assumption* recorded here, not a schedule: whatever
/// actually emits heartbeats lives in the application and adapter layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LivenessWindows {
    online: DurationMillis,
    offline: DurationMillis,
}

impl LivenessWindows {
    /// Cadence at which a peer is assumed to produce evidence of life.
    pub const HEARTBEAT_INTERVAL: DurationMillis = DurationMillis::from_secs(10);

    /// Evidence younger than this means `Online`.
    pub const DEFAULT_ONLINE: DurationMillis = DurationMillis::from_secs(30);

    /// Evidence at least this old means `Offline`.
    pub const DEFAULT_OFFLINE: DurationMillis = DurationMillis::from_secs(60);

    /// The engineering defaults described on this type.
    pub const DEFAULT: Self = Self {
        online: Self::DEFAULT_ONLINE,
        offline: Self::DEFAULT_OFFLINE,
    };

    /// Builds a custom pair of windows.
    ///
    /// `online` must be strictly shorter than `offline`: with equal windows the
    /// `Stale` band vanishes and a peer would jump from `Online` to `Offline`
    /// with no interval in which the local view admits it does not know.
    pub const fn new(
        online: DurationMillis,
        offline: DurationMillis,
    ) -> Result<Self, LivenessWindowsError> {
        if online.as_millis() >= offline.as_millis() {
            return Err(LivenessWindowsError::WindowsNotOrdered);
        }

        Ok(Self { online, offline })
    }

    pub const fn online(&self) -> DurationMillis {
        self.online
    }

    pub const fn offline(&self) -> DurationMillis {
        self.offline
    }
}

impl Default for LivenessWindows {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Typed construction error for [`LivenessWindows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessWindowsError {
    /// The online window is not strictly shorter than the offline window.
    WindowsNotOrdered,
}

impl fmt::Display for LivenessWindowsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowsNotOrdered => {
                f.write_str("online liveness window must be shorter than the offline window")
            }
        }
    }
}

impl std::error::Error for LivenessWindowsError {}
