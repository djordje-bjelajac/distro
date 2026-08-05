//! `messaging` bounded context: conversations, messages, ordering, and delivery state.

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod test_peers;
