use std::collections::BTreeMap;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use identity::domain::VerificationState;
use identity::ports::{TrustRecordStoreError, TrustRecordStorePort};
use messaging::ports::AuthorPolicyPort;
use shared_types::PeerId;

/// The composition-root wiring of invariant 11, and the roster's trust badges,
/// over one cached read of `identity`'s trust records.
///
/// # Why this type exists at all
///
/// `messaging` declares `AuthorPolicyPort` — "is content from this peer
/// refused?" — because invariant 11 had no enforcement site in that context.
/// The list it consults is `identity`'s `TrustRecord` block flag. Neither
/// context may import the other and `shared_types` hosts no port traits (canvas
/// §2.4, §4), so something outside both has to join them, and that something is
/// the composition root. `infra-sim-net`'s `TrustRecordAuthorPolicy` is the
/// same wiring over the simulator's in-memory records; this is its production
/// counterpart, and the two must behave alike or a scenario would prove
/// something about the simulator only.
///
/// # Why it caches, and why that is the port's own instruction
///
/// `AuthorPolicyPort::is_blocked` has **no error type**, and its documentation
/// says why and what to do about it:
///
/// > *An implementation that would have to read a file or a network to answer
/// > must load the list ahead of time instead: a failure here has no safe
/// > default, since blocking everyone silences the network and blocking no one
/// > ignores the user's decision.*
///
/// The production trust store is a file (S4), so answering from it directly
/// would be exactly the thing that paragraph forbids — and it would put a
/// synchronous file read on the path of every inbound envelope. So the list is
/// loaded ahead of time, here, and [`refresh`](Self::refresh) is called by the
/// root: on startup, on every tick, and immediately after any command that
/// changes trust, so a block a user just made takes effect on the next envelope
/// rather than on the next tick.
///
/// A refresh that fails leaves the previous snapshot in place and reports the
/// error to its caller. That is the only honest choice: the last known list is
/// the user's most recent decision, and discarding it because a read failed
/// would silently unblock everyone.
///
/// # Why it also serves the roster
///
/// The UI needs a verified/blocked badge per peer on every redraw (AC6,
/// invariant 11). Reading that through `IdentityQueryPort::peer_trust_state`
/// would be one whole-file read per peer per frame. Both axes come from the
/// same records, so one snapshot serves both — and there is then one answer in
/// the process about whether a peer is blocked, rather than a pane and a policy
/// that can disagree.
pub struct TrustDirectory {
    records: Arc<dyn TrustRecordStorePort + Send + Sync>,
    snapshot: RwLock<BTreeMap<PeerId, PeerTrust>>,
}

impl TrustDirectory {
    /// A directory over `records`, holding nothing until it is refreshed.
    ///
    /// Empty is the trust-on-first-use starting point for every peer, so an
    /// instance whose first refresh has not run yet blocks nobody — the same
    /// state a fresh install is in.
    pub fn new(records: Arc<dyn TrustRecordStorePort + Send + Sync>) -> Self {
        Self {
            records,
            snapshot: RwLock::new(BTreeMap::new()),
        }
    }

    /// Re-reads the block list and the verification state of `peers`.
    ///
    /// Both axes are read because they are orthogonal (`TrustRecord`): a peer
    /// may be verified and blocked at once, and a snapshot carrying only one of
    /// them would make the UI invent a combined state the domain does not have.
    ///
    /// Every blocked peer is included whether or not it is in `peers` — a
    /// blocked peer that has left the roster is still blocked, and dropping it
    /// from the snapshot would unblock it the moment it went offline.
    pub fn refresh(&self, peers: &[PeerId]) -> Result<(), TrustRecordStoreError> {
        let blocked = self.records.list_blocked_peers()?;

        let mut snapshot = BTreeMap::new();
        for peer in blocked.iter().chain(peers) {
            if snapshot.contains_key(peer) {
                continue;
            }
            let trust =
                self.records
                    .load_trust_record(*peer)?
                    .map_or_else(PeerTrust::default, |record| PeerTrust {
                        verification: record.verification(),
                        blocked: record.is_blocked(),
                    });

            snapshot.insert(*peer, trust);
        }

        *self.write() = snapshot;
        Ok(())
    }

    /// What this peer locally believes about `peer`.
    ///
    /// A peer with no record is the trust-on-first-use starting point —
    /// unverified and not blocked — which matches `IdentityQueryPort`'s own
    /// answer for an unknown peer.
    pub fn trust_of(&self, peer: PeerId) -> PeerTrust {
        self.read().get(&peer).copied().unwrap_or_default()
    }

    /// Every peer this instance is currently dropping traffic from, in
    /// `PeerId` order.
    pub fn blocked_peers(&self) -> Vec<PeerId> {
        self.read()
            .iter()
            .filter(|(_, trust)| trust.blocked)
            .map(|(peer, _)| *peer)
            .collect()
    }

    /// A poisoned lock means a previous holder panicked. The map has no
    /// invariant a panic could have broken — it is replaced wholesale by
    /// [`refresh`](Self::refresh) — so recovering is correct, and refusing to
    /// would take the whole application down for a bug elsewhere.
    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<PeerId, PeerTrust>> {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<PeerId, PeerTrust>> {
        self.snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl AuthorPolicyPort for TrustDirectory {
    fn is_blocked(&self, peer: PeerId) -> bool {
        // No I/O and nothing to fail, exactly as the port requires: the list
        // was loaded ahead of time.
        self.trust_of(peer).blocked
    }
}

impl std::fmt::Debug for TrustDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrustDirectory")
            .field("peers", &self.read().len())
            .finish_non_exhaustive()
    }
}

/// The two orthogonal trust axes for one peer, as the snapshot holds them.
///
/// Separate fields rather than one combined state, matching `TrustRecord`:
/// verification answers "is this key really theirs?", blocking answers "do I
/// want their traffic?", and a blocked *and* verified peer is an ordinary
/// thing a UI must be able to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerTrust {
    pub verification: VerificationState,
    pub blocked: bool,
}

impl PeerTrust {
    pub const fn is_verified(&self) -> bool {
        matches!(self.verification, VerificationState::Verified)
    }
}
