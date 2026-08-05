use crate::trace::TraceEvent;

/// One line of an [`EventTrace`](crate::trace::EventTrace): what happened, and
/// when on the virtual clock.
///
/// The instant is carried because a scenario's most common question after
/// "what happened" is "how long did it take", and reading it off the trace
/// beats threading a clock through every assertion. It is a virtual-clock
/// reading, so it is part of what a determinism comparison pins: two runs that
/// produced the same events at different instants are *not* the same run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    /// The virtual-clock instant this happened at, in milliseconds.
    pub at: u64,
    /// What happened.
    pub event: TraceEvent,
}
