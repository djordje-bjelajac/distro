use shared_types::PeerId;

/// Which conversation a message belongs to (canvas §2.3).
///
/// The network has exactly one broadcast channel — readable by every member by
/// design (D3) — and one direct conversation per remote peer. There is no room
/// name, no channel registry, and no membership list: `Broadcast` is a
/// singleton and a `Direct` conversation is fully identified by its
/// counterpart's `PeerId`.
///
/// A `PeerId` is the only addressing concept this context has. It never learns
/// what an `Endpoint` is (canvas §4); who can be reached, and how, is
/// `membership`'s business, reported here only as a `PeerId` becoming
/// connected or disconnected.
///
/// `Ord` is derived so conversation listings and log iteration are
/// deterministic (AC13); `Broadcast` sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConversationId {
    /// The single network-wide channel every member can read (D3, AC10).
    Broadcast,
    /// The 1:1 conversation with one remote peer (D4).
    Direct(PeerId),
}

impl ConversationId {
    pub const fn is_broadcast(&self) -> bool {
        matches!(self, Self::Broadcast)
    }

    pub const fn is_direct(&self) -> bool {
        matches!(self, Self::Direct(_))
    }

    /// The remote peer of a direct conversation; `None` for the broadcast
    /// channel, which has no single counterpart.
    pub const fn counterpart(&self) -> Option<PeerId> {
        match self {
            Self::Broadcast => None,
            Self::Direct(peer) => Some(*peer),
        }
    }
}
