/// How an [`Endpoint`](crate::domain::Endpoint) is reached (canvas §2.2).
///
/// The distinction is domain-relevant rather than cosmetic: a relayed endpoint
/// depends on a *third peer* volunteering circuit service (D2, AC12), so it is
/// the path that disappears first, costs another peer's bandwidth, and the one
/// a UI must be able to name when it explains why a peer is unreachable (S7).
/// The relay carries ciphertext either way, so this says nothing about
/// confidentiality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reachability {
    /// Reached by dialling the address itself.
    Direct,
    /// Reached through another peer acting as a circuit relay.
    Relayed,
}

impl Reachability {
    pub const fn is_relayed(self) -> bool {
        matches!(self, Self::Relayed)
    }
}
