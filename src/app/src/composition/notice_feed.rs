use std::collections::VecDeque;
use std::sync::Mutex;

/// The running account of what this instance did and what refused it.
///
/// # Why a terminal application needs one
///
/// AC3 requires that a failed join "produces a visible diagnostic, never a
/// hang", and the diagnostic is a multi-line `JoinDiagnostic` naming every rung
/// tried. There is nowhere else to put it: `stderr` is the screen the TUI is
/// drawing on, and a status line has room for one word. Everything a user needs
/// to be told but cannot be shown in a pane — the join account, a transport
/// refusal, the minted ticket — arrives here and is rendered as a scrollback.
///
/// Bounded and newest-last: an instance running for a day must not accumulate
/// notices without limit, and the oldest are the ones already read.
#[derive(Debug)]
pub struct NoticeFeed {
    notices: Mutex<VecDeque<Notice>>,
    capacity: usize,
}

impl NoticeFeed {
    /// Notices kept for the log pane.
    pub const DEFAULT_CAPACITY: usize = 200;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            notices: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
        }
    }

    /// Records something worth knowing.
    pub fn push(&self, level: NoticeLevel, text: impl Into<String>) {
        let mut notices = self.lock();

        while notices.len() >= self.capacity {
            notices.pop_front();
        }
        notices.push_back(Notice {
            level,
            text: text.into(),
        });
    }

    /// Records something ordinary.
    pub fn info(&self, text: impl Into<String>) {
        self.push(NoticeLevel::Info, text);
    }

    /// Records something that refused, failed, or was dropped.
    pub fn warn(&self, text: impl Into<String>) {
        self.push(NoticeLevel::Warning, text);
    }

    /// Every notice held, oldest first.
    pub fn all(&self) -> Vec<Notice> {
        self.lock().iter().cloned().collect()
    }

    /// The most recent `count` notices, oldest first.
    pub fn latest(&self, count: usize) -> Vec<Notice> {
        let notices = self.lock();
        let start = notices.len().saturating_sub(count);

        notices.iter().skip(start).cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Notice>> {
        self.notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for NoticeFeed {
    fn default() -> Self {
        Self::new()
    }
}

/// One line in the log pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub level: NoticeLevel,
    pub text: String,
}

/// How loudly a notice should read.
///
/// Two levels, not five: on one pane the only decision a colour makes is
/// whether the eye should stop, and a scale invites levels nobody can tell
/// apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warning,
}
