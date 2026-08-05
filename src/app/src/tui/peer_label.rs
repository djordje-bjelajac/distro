use shared_types::{Fingerprint, PeerId};

/// How a peer is named on screen.
///
/// # Why not a display name
///
/// `DisplayName` is `identity`'s, and it belongs to the **local** peer only.
/// Nothing in this workspace stores a remote peer's chosen name, and that is
/// deliberate: invariant 8 says a display name never participates in identity,
/// equality, addressing, or lookup, and a roster that showed a name a stranger
/// chose for themselves would be showing exactly the field an impersonator
/// would set. Two peers may freely pick the same one.
///
/// So a remote peer is labelled by the leading characters of its
/// [`Fingerprint`] — the same digest a user compares out of band to verify it
/// (AC6). The label is therefore something that *cannot* be forged into
/// something else's, which is the property a roster needs.
///
/// The local peer is labelled `you`, because a user reading their own
/// fingerprint back at themselves learns nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerLabels {
    local: PeerId,
}

impl PeerLabels {
    /// Fingerprint characters shown in a label, including the separating
    /// space: `21fe 31df` is two groups, which is 32 bits — enough to tell a
    /// handful of peers apart at a glance, and short enough for a roster
    /// column. The **full** digest is what a user compares before verifying,
    /// and it is shown in the fingerprint overlay rather than here.
    const LABEL_CHARACTERS: usize = 9;

    pub const fn for_local(local: PeerId) -> Self {
        Self { local }
    }

    /// What to call `peer` on screen.
    pub fn label(&self, peer: PeerId) -> String {
        if peer == self.local {
            return "you".to_owned();
        }

        Self::short(peer)
    }

    /// Whether `peer` is this instance.
    ///
    /// `PeerId`'s own equality is key equality (invariant 1), so this cannot be
    /// fooled by anything a peer says about itself.
    pub fn is_local(&self, peer: PeerId) -> bool {
        peer == self.local
    }

    /// The leading characters of a peer's fingerprint.
    pub fn short(peer: PeerId) -> String {
        Fingerprint::of(&peer)
            .to_string()
            .chars()
            .take(Self::LABEL_CHARACTERS)
            .collect()
    }

    /// The full digest a user reads aloud to verify a peer out of band (AC6).
    pub fn full_fingerprint(peer: PeerId) -> String {
        Fingerprint::of(&peer).to_string()
    }
}
