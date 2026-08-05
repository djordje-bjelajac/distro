//! The one source of randomness in this crate (S5).
//!
//! Everything a simulated network could decide by chance — a shuffled gossip
//! order, a peer picked out of a set — draws from [`SeededRng`], so a scenario
//! is fully described by its seed and its script. `rand` is deliberately not a
//! dependency: its output is not a stable contract across versions, and a
//! recorded trace that stops matching after `cargo update` would make the
//! determinism guarantee worthless.

mod seeded_rng;
#[cfg(test)]
mod seeded_rng_test;

pub use seeded_rng::SeededRng;
