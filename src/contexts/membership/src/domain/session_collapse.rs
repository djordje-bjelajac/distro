use std::cmp::Ordering;
use std::fmt;

use shared_types::PeerId;

use crate::domain::{Session, SessionDirection, SessionState};

/// The deterministic outcome of a simultaneous connect between one peer pair
/// (canvas §2.5, invariant 3).
///
/// # The rule
///
/// Two peers that dial each other at the same time end up with two links where
/// they wanted one. Both must discard the same one, and they must do it
/// **without exchanging a message** — any negotiation would need a link, which
/// is the thing in question, and would be one more round trip to lose.
///
/// The rule is therefore a pure function of the two identities:
/// *the session initiated by the lexicographically lower `PeerId` survives*.
/// Both sides evaluate it over the same two keys and reach the same answer;
/// each merely translates that answer into its own [`SessionDirection`], since
/// one side's outbound session is the other side's inbound one.
///
/// The ordering is `PeerId`'s derived `Ord` over its Ed25519 public-key bytes.
/// That ordering is pinned by `shared_types` precisely because this rule stands
/// on it: changing it would split a running network into peers that disagree
/// about which link to keep.
///
/// This is the **normal case**, not an edge case — in a symmetric network where
/// every peer dials every peer it discovers, simultaneous connects happen
/// constantly.
///
/// # What it does not decide
///
/// Nothing about *when* to apply it, and nothing about the transport. Closing
/// the superseded link is the application's job via `PeerTransportPort`; this
/// type only says which one it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCollapse {
    initiator: PeerId,
    survivor: SessionDirection,
}

impl SessionCollapse {
    /// Applies the rule to a peer pair.
    ///
    /// `local` and `remote` must differ: two identical keys have no lower one,
    /// and the pair should never have existed (invariant 2).
    pub fn resolve(local: PeerId, remote: PeerId) -> Result<Self, SessionCollapseError> {
        match local.cmp(&remote) {
            Ordering::Equal => Err(SessionCollapseError::SelfConnection),
            // We are the lower peer, so the session we dialled survives.
            Ordering::Less => Ok(Self {
                initiator: local,
                survivor: SessionDirection::Outbound,
            }),
            // They are the lower peer, so the session they dialled survives.
            Ordering::Greater => Ok(Self {
                initiator: remote,
                survivor: SessionDirection::Inbound,
            }),
        }
    }

    /// Applies the rule to two concrete live sessions with the same remote,
    /// one per direction.
    ///
    /// Argument order is irrelevant. Rejects anything that is not actually a
    /// simultaneous connect: two sessions with different remotes, two sessions
    /// in the same direction, or a session that is no longer live — in each of
    /// those cases the caller's premise is wrong, and collapsing would discard
    /// a link for no reason.
    pub fn between(
        local: PeerId,
        first: &Session,
        second: &Session,
    ) -> Result<Self, SessionCollapseError> {
        if first.remote() != second.remote() {
            return Err(SessionCollapseError::RemoteMismatch);
        }
        for session in [first, second] {
            if !session.is_live() {
                return Err(SessionCollapseError::SessionNotLive {
                    state: session.state(),
                });
            }
        }
        if first.direction() == second.direction() {
            return Err(SessionCollapseError::SameDirection);
        }

        Self::resolve(local, first.remote())
    }

    /// The peer whose dial survives: the lower of the two `PeerId`s.
    pub const fn initiator(&self) -> PeerId {
        self.initiator
    }

    /// The surviving session, named from the local peer's point of view.
    pub const fn survivor(&self) -> SessionDirection {
        self.survivor
    }

    /// The session the local peer must now close.
    pub const fn superseded(&self) -> SessionDirection {
        self.survivor.opposite()
    }
}

/// Typed rejection of a [`SessionCollapse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCollapseError {
    /// The two identities are the same key (invariant 2).
    SelfConnection,
    /// The two sessions are not with the same remote peer.
    RemoteMismatch,
    /// The two sessions share a direction, so no simultaneous connect occurred.
    SameDirection,
    /// One of the sessions is no longer live.
    SessionNotLive { state: SessionState },
}

impl fmt::Display for SessionCollapseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfConnection => {
                f.write_str("a session pair cannot be collapsed against the local peer itself")
            }
            Self::RemoteMismatch => {
                f.write_str("the two sessions are not with the same remote peer")
            }
            Self::SameDirection => f.write_str(
                "the two sessions have the same direction, so no simultaneous connect occurred",
            ),
            Self::SessionNotLive { state } => {
                write!(
                    f,
                    "a session in state {state} cannot take part in a collapse"
                )
            }
        }
    }
}

impl std::error::Error for SessionCollapseError {}
