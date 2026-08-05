//! Command handlers: the paths that change `identity` state.
//!
//! Each command is an imperative DTO naming a use case, each handler is named
//! by intent, and every handler returns the past-tense event its change
//! produced — or `None`/a typed error when nothing changed. Nothing here
//! returns a read model; that is [`queries`](crate::application::queries).
//!
//! The commands live here rather than in `ports/` because a port may depend on
//! `domain` and `shared_types` only. `IdentityCommandPort` therefore speaks in
//! domain types, and [`IdentityCommandService`] builds these DTOs from them.

mod block_peer;
#[cfg(test)]
mod block_peer_test;
mod identity_command_service;
mod initialize_local_identity;
#[cfg(test)]
mod initialize_local_identity_test;
mod set_display_name;
#[cfg(test)]
mod set_display_name_test;
mod unblock_peer;
#[cfg(test)]
mod unblock_peer_test;
mod verify_peer;
#[cfg(test)]
mod verify_peer_test;

pub use block_peer::{BlockPeer, BlockPeerHandler};
pub use identity_command_service::IdentityCommandService;
pub use initialize_local_identity::{InitializeLocalIdentity, InitializeLocalIdentityHandler};
pub use set_display_name::{SetDisplayName, SetDisplayNameHandler};
pub use unblock_peer::{UnblockPeer, UnblockPeerHandler};
pub use verify_peer::{VerifyPeer, VerifyPeerHandler};
