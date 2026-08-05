//! The hostile-input caps of canvas §7/S6, enforced at this boundary.
//!
//! An open network has no gatekeeper to add these later, so they ship from v1
//! and they are enforced *here* — the last place between a stranger's bytes and
//! this process's memory. Where a cap can be checked before deserialization it
//! is (invariant 12): the wire framing refuses an oversize frame from its
//! length prefix alone.

mod inbound_rate_limiter;
#[cfg(test)]
mod inbound_rate_limiter_test;
mod resource_limits;

pub use inbound_rate_limiter::InboundRateLimiter;
pub use resource_limits::ResourceLimits;
