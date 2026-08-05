use std::fmt;

use shared_types::PeerId;

use crate::domain::{SessionDirection, SessionState};

/// One authenticated link to one remote peer (canvas §2.2).
///
/// `Connecting → Established → Closed`, with `Closed` terminal. The entity
/// carries no socket, no stream, and no transport handle: it is the local
/// record of what the link *is*, while the link itself lives behind
/// `PeerTransportPort`.
///
/// Invariant 2 is enforced in the constructor: a session whose remote is the
/// local peer's own `PeerId` cannot be built, so no later code needs to guard
/// against a peer talking to itself. That case is not hypothetical — a peer's
/// own announcement comes back from discovery, and a join ticket can be pasted
/// into the machine that minted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    remote: PeerId,
    direction: SessionDirection,
    state: SessionState,
}

impl Session {
    /// Opens a session in [`SessionState::Connecting`].
    ///
    /// Rejects `remote == local` with [`SessionError::SelfConnection`]
    /// (invariant 2).
    pub fn open(
        local: PeerId,
        remote: PeerId,
        direction: SessionDirection,
    ) -> Result<Self, SessionError> {
        if remote == local {
            return Err(SessionError::SelfConnection);
        }

        Ok(Self {
            remote,
            direction,
            state: SessionState::Connecting,
        })
    }

    pub const fn remote(&self) -> PeerId {
        self.remote
    }

    pub const fn direction(&self) -> SessionDirection {
        self.direction
    }

    pub const fn state(&self) -> SessionState {
        self.state
    }

    pub const fn is_live(&self) -> bool {
        self.state.is_live()
    }

    pub const fn is_established(&self) -> bool {
        matches!(self.state, SessionState::Established)
    }

    /// Records that the handshake completed.
    ///
    /// Legal only from `Connecting`. Re-establishing an established session is
    /// rejected rather than ignored: the caller believes a handshake it did not
    /// observe, and `PeerConnected` must be published exactly once per link.
    pub fn establish(&mut self) -> Result<(), SessionError> {
        if !matches!(self.state, SessionState::Connecting) {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Established,
            });
        }

        self.state = SessionState::Established;
        Ok(())
    }

    /// Ends the session from either live state.
    ///
    /// Closing an already-closed session is rejected for the mirror reason:
    /// `PeerDisconnected` must not be published twice for one link.
    pub fn close(&mut self) -> Result<(), SessionError> {
        if !self.state.is_live() {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Closed,
            });
        }

        self.state = SessionState::Closed;
        Ok(())
    }
}

/// Typed rejection of a [`Session`] construction or transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The session's remote is the local peer itself (invariant 2).
    SelfConnection,
    /// The requested transition is not legal from the current state.
    InvalidTransition {
        from: SessionState,
        to: SessionState,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfConnection => {
                f.write_str("a session cannot be opened to the local peer itself")
            }
            Self::InvalidTransition { from, to } => {
                write!(f, "session cannot move from {from} to {to}")
            }
        }
    }
}

impl std::error::Error for SessionError {}
