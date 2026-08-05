use shared_types::{Fingerprint, PayloadKind, PeerId, ProtocolVersion};

use crate::domain::events::{DisplayNameChanged, LocalIdentityInitialized};
use crate::domain::{DisplayName, UnsignedEnvelope};

/// The peer running this process: its stable [`PeerId`] and its
/// [`DisplayName`] (canvas §2.1).
///
/// # No key material lives here
///
/// The aggregate holds a *public* identity only. The keypair belongs to
/// `IdentityKeyStorePort`, which hands back nothing but the `PeerId`, and
/// signing belongs to `EnvelopeSignerPort`. Secret bytes therefore never cross
/// a port boundary in either direction and cannot be reached through this
/// type — there is no accessor that could leak them, because there is nothing
/// to leak.
///
/// That is why the aggregate signs *indirectly*: it produces an
/// [`UnsignedEnvelope`] authored by itself via
/// [`draft_envelope`](Self::draft_envelope), and the signer port is the only
/// thing that can complete it. Keeping the trait in `ports/` and the draft in
/// `domain/` is what lets dependencies keep pointing inward — the domain never
/// names a port.
///
/// Per invariant 8 the display name takes no part in identity, so this type
/// deliberately implements no `PartialEq`: two `LocalIdentity` values are
/// "the same peer" exactly when their [`peer_id`](Self::peer_id) match, and
/// nothing else.
#[derive(Debug, Clone)]
pub struct LocalIdentity {
    peer: PeerId,
    display_name: DisplayName,
}

impl LocalIdentity {
    /// Assumes the local identity for this process, returning the aggregate
    /// and the event announcing it.
    ///
    /// The `PeerId` comes from `IdentityKeyStorePort`'s load-or-create call,
    /// so the domain cannot distinguish a first launch from a later one — and
    /// by AC9 it must not: the observable identity is the same either way.
    pub fn initialize(peer: PeerId, display_name: DisplayName) -> (Self, LocalIdentityInitialized) {
        let event = LocalIdentityInitialized {
            peer,
            display_name: display_name.clone(),
        };

        (Self { peer, display_name }, event)
    }

    pub const fn peer_id(&self) -> PeerId {
        self.peer
    }

    pub const fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    /// The digest a user reads aloud to a peer to move that peer's
    /// `TrustRecord` from `Unverified` to `Verified` (D5).
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&self.peer)
    }

    /// Renames the local peer.
    ///
    /// Returns the emitted event, or `None` when `new_name` is the name the
    /// peer already has: no change occurred, so no change is announced. This
    /// keeps a repeated `SetDisplayName` command harmless. Because
    /// `DisplayName` is stored trimmed, differently padded spellings of the
    /// same name compare equal here.
    pub fn change_display_name(&mut self, new_name: DisplayName) -> Option<DisplayNameChanged> {
        if self.display_name == new_name {
            return None;
        }

        let previous = std::mem::replace(&mut self.display_name, new_name);

        Some(DisplayNameChanged {
            peer: self.peer,
            previous,
            current: self.display_name.clone(),
        })
    }

    /// Drafts an envelope authored by this peer at the protocol version this
    /// build speaks.
    ///
    /// The author is set from the local `PeerId` and can never be supplied by
    /// a caller or read out of a payload (invariant 4). `payload` stays opaque
    /// bytes: encoding is an adapter concern, and the version is always
    /// [`ProtocolVersion::CURRENT`] because a peer only ever speaks its own
    /// (S2 — peers upgrade independently).
    pub fn draft_envelope(&self, kind: PayloadKind, payload: Vec<u8>) -> UnsignedEnvelope {
        UnsignedEnvelope::draft(self.peer, ProtocolVersion::CURRENT, kind, payload)
    }
}
