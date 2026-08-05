use crate::domain::Millis;

/// The one source of time for this context (D11, S5).
///
/// Every time-dependent rule — presence expiry, join-ticket validity, cache
/// pruning — is a pure function that takes an instant, and this port is the
/// only thing that produces one. That is what makes AC13 achievable: domain
/// and application tests advance a fake clock by hand and never wait, and the
/// sim-net harness (OP-8) can drive a whole multi-peer scenario deterministically.
///
/// # No error type
///
/// Reading a monotonic counter cannot fail, and giving it a `Result` would push
/// a meaningless error branch into every caller. An adapter that somehow cannot
/// read a clock has no sensible fallback anyway.
///
/// # Contract
///
/// Successive calls never return a smaller value: the readings must come from a
/// monotonic source, not a wall clock that a user or NTP can move backwards.
/// The domain still saturates rather than trusting this blindly, but a
/// regressing implementation makes every age meaningless.
pub trait ClockPort {
    /// The current instant on this peer's monotonic timeline.
    fn now(&self) -> Millis;
}
