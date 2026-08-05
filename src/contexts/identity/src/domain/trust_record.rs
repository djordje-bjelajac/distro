use std::fmt;

use shared_types::PeerId;

use crate::domain::VerificationState;
use crate::domain::events::{PeerBlocked, PeerUnblocked, PeerVerified};

/// Everything this peer locally believes about one remote [`PeerId`]: how far
/// that peer has climbed the trust-on-first-use ladder, and whether the local
/// user has blocked it (canvas §2.1).
///
/// The two are **orthogonal** by design. Verification answers "is this key
/// really theirs?"; blocking answers "do I want their traffic?". Blocking a
/// verified peer therefore preserves the verification, and unblocking restores
/// the peer to exactly the verification state it held before — the flag never
/// touched it. Every record is authoritative for this peer alone (invariant 9)
/// and blocking is purely local: nothing is announced (invariant 11).
///
/// # Idempotent vs. rejected transitions
///
/// [`verify`](Self::verify) is **idempotent**: re-verifying an already
/// verified peer succeeds, changes nothing, and emits no event. Verification
/// asserts a fact the user established out-of-band by comparing fingerprints,
/// and a user may well repeat that comparison. The command's post-condition
/// ("this peer is verified") already holds, so there is nothing to report as
/// failure, and the transition is monotonic — no later call can contradict an
/// earlier one. Emitting no second event keeps subscribers from seeing a
/// transition that did not occur.
///
/// [`block`](Self::block) and [`unblock`](Self::unblock) are **rejected** when
/// they would not change the flag. They are inverse commands whose entire
/// meaning is the flip; issuing one against a state where it is meaningless
/// means the caller's view of this record is stale, which is precisely what a
/// typed error should surface rather than swallow. Silently succeeding would
/// let a UI report "blocked" for an action that did nothing, and would hide
/// double-dispatch bugs in the application layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustRecord {
    peer: PeerId,
    verification: VerificationState,
    blocked: bool,
}

impl TrustRecord {
    /// The trust-on-first-use starting point for a newly seen peer: known,
    /// unverified, not blocked (D5).
    pub const fn unverified(peer: PeerId) -> Self {
        Self {
            peer,
            verification: VerificationState::Unverified,
            blocked: false,
        }
    }

    /// Rebuilds a record from previously stored state.
    ///
    /// Every combination of the two axes is legal precisely because they are
    /// orthogonal, so this cannot fail.
    pub const fn rehydrate(peer: PeerId, verification: VerificationState, blocked: bool) -> Self {
        Self {
            peer,
            verification,
            blocked,
        }
    }

    /// The remote peer this record is about.
    pub const fn peer(&self) -> PeerId {
        self.peer
    }

    pub const fn verification(&self) -> VerificationState {
        self.verification
    }

    pub const fn is_verified(&self) -> bool {
        self.verification.is_verified()
    }

    pub const fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Records an out-of-band fingerprint confirmation.
    ///
    /// Returns the emitted event on the `Unverified → Verified` transition and
    /// `None` when the peer was already verified (see the type-level note on
    /// idempotency). Independent of the blocked flag: a user may confirm the
    /// key of a peer whose traffic they are dropping.
    pub fn verify(&mut self) -> Option<PeerVerified> {
        if self.verification.is_verified() {
            return None;
        }

        self.verification = VerificationState::Verified;
        Some(PeerVerified { peer: self.peer })
    }

    /// Blocks the peer locally, leaving verification untouched.
    pub fn block(&mut self) -> Result<PeerBlocked, TrustRecordError> {
        if self.blocked {
            return Err(TrustRecordError::AlreadyBlocked);
        }

        self.blocked = true;
        Ok(PeerBlocked { peer: self.peer })
    }

    /// Unblocks the peer, which returns to the verification state it kept
    /// throughout.
    pub fn unblock(&mut self) -> Result<PeerUnblocked, TrustRecordError> {
        if !self.blocked {
            return Err(TrustRecordError::NotBlocked);
        }

        self.blocked = false;
        Ok(PeerUnblocked { peer: self.peer })
    }
}

/// Typed rejection of a [`TrustRecord`] transition that would change nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustRecordError {
    /// `block` was issued against a peer that is already blocked.
    AlreadyBlocked,
    /// `unblock` was issued against a peer that is not blocked.
    NotBlocked,
}

impl fmt::Display for TrustRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyBlocked => f.write_str("peer is already blocked"),
            Self::NotBlocked => f.write_str("peer is not blocked"),
        }
    }
}

impl std::error::Error for TrustRecordError {}
