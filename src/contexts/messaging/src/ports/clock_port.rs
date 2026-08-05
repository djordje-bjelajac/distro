use crate::domain::Millis;

/// The one source of time for this context (D11, S5).
///
/// It produces exactly one thing: the instant this peer stamps on a message it
/// is sending, for display. Nothing in this context orders, expires, or dedups
/// by time — [`SequenceNumber`](crate::domain::SequenceNumber) does the
/// ordering (invariant 5) — so a wrong clock here makes a timestamp wrong and
/// nothing else. That is deliberate: on a network of unsynchronised peers, any
/// rule that depended on comparing clocks would be a rule that could be broken
/// by lying about the time.
///
/// # A duplicate by design
///
/// `membership` declares a `ClockPort` too. `shared_types` is a data-contract
/// crate that hosts no port (canvas §2.4), and importing another context's
/// trait would be a cross-context import (canvas §4) — so each context declares
/// the time it needs in its own terms and the composition root wires them to
/// one implementation.
///
/// # No error type
///
/// Reading a counter cannot fail, and a `Result` would push a meaningless error
/// branch into every caller.
///
/// # Contract
///
/// Successive calls never return a smaller value.
pub trait ClockPort {
    /// The current instant on this peer's timeline.
    fn now(&self) -> Millis;
}
