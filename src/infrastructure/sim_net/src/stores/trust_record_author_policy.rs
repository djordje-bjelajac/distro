use std::sync::Arc;

use messaging::ports::AuthorPolicyPort;
use shared_types::PeerId;

use crate::stores::InMemoryTrustRecords;

/// The composition-root wiring of invariant 11: `messaging` asks its own
/// question, `identity` holds the answer (canvas §4).
///
/// # Why this type exists at all
///
/// `messaging` declares `AuthorPolicyPort` — "is content from this peer
/// refused?" — because invariant 11 had no enforcement site in that context.
/// The list it consults is `identity`'s `TrustRecord` block flag. Neither
/// context may import the other and `shared_types` hosts no port traits, so
/// something outside both has to join them. On a real launch that is the
/// composition root (OP-12); in a scenario it is this adapter, and it is wired
/// automatically for every simulated peer.
///
/// The consequence a scenario can rely on: blocking a peer through
/// `identity`'s command port makes `messaging` refuse that peer's next envelope
/// with `AuthorBlocked`, with nothing else to arrange.
///
/// # Blocking is local, and says nothing
///
/// Nothing is announced to the blocked peer or to anyone else. It keeps
/// sending; this peer stops listening.
pub struct TrustRecordAuthorPolicy {
    records: Arc<InMemoryTrustRecords>,
}

impl TrustRecordAuthorPolicy {
    /// A policy reading `records`.
    pub const fn new(records: Arc<InMemoryTrustRecords>) -> Self {
        Self { records }
    }
}

impl AuthorPolicyPort for TrustRecordAuthorPolicy {
    fn is_blocked(&self, peer: PeerId) -> bool {
        // The port has no error type on purpose: a failure here has no safe
        // default, since blocking everyone silences the network and blocking
        // nobody ignores the user's decision. An in-memory map cannot fail, so
        // the question is simply answered.
        self.records.is_blocked(peer)
    }
}
