//! Context-internal domain events of `identity` (canvas §2.1), all named in
//! the past tense.
//!
//! None of these is a published cross-context contract: the only events that
//! cross a context boundary are `PeerConnected`/`PeerDisconnected` in
//! `shared_types`. Nothing here carries key material or a timestamp — the
//! `identity` context owns no clock.

mod display_name_changed;
mod local_identity_initialized;
mod peer_blocked;
mod peer_unblocked;
mod peer_verified;

pub use display_name_changed::DisplayNameChanged;
pub use local_identity_initialized::LocalIdentityInitialized;
pub use peer_blocked::PeerBlocked;
pub use peer_unblocked::PeerUnblocked;
pub use peer_verified::PeerVerified;
