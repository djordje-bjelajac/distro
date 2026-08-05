//! `membership` bounded context: peer roster, sessions, presence, and network join.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod test_peers;
