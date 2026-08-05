//! The simulated timeline every peer shares (D11, S5).

mod virtual_clock;
#[cfg(test)]
mod virtual_clock_test;

pub use virtual_clock::VirtualClock;
