use shared_types::PeerId;

use crate::ports::{LocalIdentitySummary, PeerTrustState, TrustRecordStoreError};

/// The **inbound** (driving) read contract of `identity` (canvas §4, inbound
/// column).
///
/// Every method is a query: it returns a read model and changes nothing —
/// not the local identity, not a trust record, not the store. Asking about a
/// peer that was never verified or blocked writes no record; it reports the
/// trust-on-first-use starting point instead. That is the half of the CQRS
/// split `AGENTS.md` demands be kept separate end to end, and it is asserted
/// in this crate's query tests rather than left to convention.
///
/// [`blocked_peers`](Self::blocked_peers) is also the seam invariant 11 hangs
/// on: the composition root (OP-12) reads the block list here and hands it to
/// `messaging`'s own `AuthorPolicyPort`, so neither context imports the other
/// and no port trait is published in `shared_types`.
///
/// Object-safe and `&self`-taking, so a root can hold it behind
/// `Arc<dyn IdentityQueryPort + Send + Sync>`.
pub trait IdentityQueryPort {
    /// The local peer's identity, or `None` before
    /// [`initialize_local_identity`](crate::ports::IdentityCommandPort::initialize_local_identity)
    /// has run.
    fn local_identity(&self) -> Option<LocalIdentitySummary>;

    /// What this peer locally believes about `peer`.
    ///
    /// An unknown peer yields the trust-on-first-use default rather than an
    /// error, and no record is created.
    fn peer_trust_state(&self, peer: PeerId) -> Result<PeerTrustState, TrustRecordStoreError>;

    /// Every currently blocked peer, in a stable order.
    fn blocked_peers(&self) -> Result<Vec<PeerId>, TrustRecordStoreError>;
}
