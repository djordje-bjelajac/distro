//! Deterministic multi-peer integration scenarios over `infra-sim-net`
//! (canvas OP-9).
//!
//! This crate deliberately holds no library code. Its content is the `tests/`
//! directory, one file per theme:
//!
//! * `tests/joining.rs` — identity, the D1 bootstrap ladder, presence, and
//!   simultaneous connect (AC1, AC2, AC3, AC5, AC9, invariant 3).
//! * `tests/messaging.rs` — delivery, ordering, duplicates, gaps, restart
//!   (AC4, AC7, AC8, AC10, AC11, AC15, AC16).
//! * `tests/security.rs` — forged signatures, blocking, wire versions
//!   (AC6, AC14, invariants 10 and 11).
//! * `tests/resilience.rs` — relaying and partitions (AC5, AC12).
//! * `tests/determinism.rs` — the AC13 guard for this suite.
//!
//! Every scenario runs against the simulated network: no real clock, no
//! socket, no thread, no unseeded randomness (AC13, safeguard S5).
