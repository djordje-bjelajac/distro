use shared_types::{Fingerprint, PeerId};

use crate::domain::DisplayName;

/// Read model returned by
/// [`IdentityQueryPort::local_identity`](crate::ports::IdentityQueryPort::local_identity):
/// everything a UI needs to show "who am I on this network".
///
/// The [`Fingerprint`] is carried rather than left to the caller to derive
/// because it is the whole point of the query for AC6: it is the digest a user
/// reads aloud so a peer can move this identity from `Unverified` to
/// `Verified` out-of-band (D5). A read model with no key material and no
/// behaviour, so it is plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalIdentitySummary {
    /// The stable identity of this peer (AC9).
    pub peer: PeerId,
    /// The label this peer currently shows; never part of identity
    /// (invariant 8).
    pub display_name: DisplayName,
    /// The human-comparable digest of [`peer`](Self::peer) used for
    /// out-of-band verification (AC6).
    pub fingerprint: Fingerprint,
}
