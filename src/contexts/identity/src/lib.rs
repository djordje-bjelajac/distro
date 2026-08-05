//! `identity` bounded context: local identity, trust, and envelope signing.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod test_peers;
