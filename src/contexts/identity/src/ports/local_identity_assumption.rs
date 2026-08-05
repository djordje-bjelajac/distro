use shared_types::PeerId;

use crate::domain::events::LocalIdentityInitialized;

/// What
/// [`IdentityCommandPort::initialize_local_identity`](crate::ports::IdentityCommandPort::initialize_local_identity)
/// did — the two outcomes of an idempotent bootstrap.
///
/// The command is safe to issue repeatedly, and the caller can still tell the
/// two apart: only [`Assumed`](Self::Assumed) carries an event, so re-issuing
/// it announces nothing a second time. Both variants agree on the
/// [`PeerId`](Self::peer), which is the part AC9 pins.
///
/// This says nothing about *first launch vs. restart* — deliberately. The
/// keystore's load-or-create hides that distinction (AC1/AC9: the observable
/// identity is the same either way), so no variant here could honestly report
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalIdentityAssumption {
    /// This process assumed its identity now; the event announces it.
    Assumed(LocalIdentityInitialized),
    /// This process had already assumed its identity; nothing changed and no
    /// event was emitted.
    AlreadyAssumed(PeerId),
}

impl LocalIdentityAssumption {
    /// The local peer, whichever way the command turned out.
    pub const fn peer(&self) -> PeerId {
        match self {
            Self::Assumed(event) => event.peer,
            Self::AlreadyAssumed(peer) => *peer,
        }
    }

    /// The emitted event, or `None` when this call changed nothing.
    pub const fn event(&self) -> Option<&LocalIdentityInitialized> {
        match self {
            Self::Assumed(event) => Some(event),
            Self::AlreadyAssumed(_) => None,
        }
    }
}
