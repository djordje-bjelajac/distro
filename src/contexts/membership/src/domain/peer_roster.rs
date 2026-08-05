use std::collections::BTreeMap;
use std::fmt;

use shared_types::{PeerConnected, PeerDisconnected, PeerId};

use crate::domain::events::{PeerDiscovered, PeerPresenceExpired};
use crate::domain::{
    Endpoint, KnownPeer, LivenessWindows, Millis, Session, SessionCollapse, SessionDirection,
    SessionError, SessionOutcome, SessionState,
};

/// Everything one peer knows about the peers around it: the aggregate root of
/// the `membership` context (canvas §2.2).
///
/// # This view is local, and only local
///
/// Invariant 9: the roster is authoritative for this peer alone. It holds no
/// opinion about who else is connected to whom, it never assumes global state,
/// and two peers' rosters routinely disagree — which is correct, not a
/// convergence bug to fix.
///
/// # The local peer is never an entry
///
/// Invariant 2 is enforced at every entry point rather than at the one place a
/// self-entry seems likely: a peer's own announcement genuinely comes back from
/// discovery, and its own join ticket can be pasted into the machine that
/// minted it. `SelfConnection` is therefore an expected rejection on a normal
/// path, not a defensive check against a caller bug.
///
/// # Ports are absent on purpose
///
/// Nothing here reads a clock, opens a socket, or publishes an event. Every
/// time-dependent operation takes the instant as an argument (D11, S5), and
/// every consequence a transition has beyond the roster's own state — closing a
/// superseded link, publishing `PeerConnected` — comes back in a
/// [`SessionOutcome`] for the application to carry out.
///
/// Entries are keyed in `PeerId` order, so every iteration, expiry sweep, and
/// cache write is deterministic (AC13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRoster {
    local: PeerId,
    peers: BTreeMap<PeerId, KnownPeer>,
}

impl PeerRoster {
    /// An empty roster belonging to `local`.
    pub const fn for_local_peer(local: PeerId) -> Self {
        Self {
            local,
            peers: BTreeMap::new(),
        }
    }

    /// The peer this roster belongs to.
    pub const fn local_peer(&self) -> PeerId {
        self.local
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// One entry, if the peer is known.
    pub fn peer(&self, peer: &PeerId) -> Option<&KnownPeer> {
        self.peers.get(peer)
    }

    /// Every known peer, in `PeerId` order.
    pub fn known_peers(&self) -> impl Iterator<Item = &KnownPeer> {
        self.peers.values()
    }

    /// Peers with an established session — the input to
    /// [`NetworkStatus`](crate::domain::NetworkStatus).
    pub fn established_session_count(&self) -> usize {
        self.peers
            .values()
            .filter(|entry| entry.is_connected())
            .count()
    }

    /// Records that `peer` was discovered at `at`, reachable at `endpoints`.
    ///
    /// Returns [`PeerDiscovered`] only the first time the peer is seen; later
    /// sightings merge new addresses and refresh the evidence of life, because
    /// in a gossiping network the same peer is re-announced continually and
    /// every announcement after the first carries no news.
    pub fn record_discovery(
        &mut self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
        at: Millis,
    ) -> Result<Option<PeerDiscovered>, PeerRosterError> {
        self.reject_local(peer)?;
        if endpoints.is_empty() {
            return Err(PeerRosterError::NoEndpoints);
        }

        match self.peers.get_mut(&peer) {
            Some(entry) => {
                entry.merge_endpoints(endpoints);
                entry.record_evidence(at);
                Ok(None)
            }
            None => {
                self.peers
                    .insert(peer, KnownPeer::discovered(peer, endpoints, at));
                Ok(Some(PeerDiscovered { peer, at }))
            }
        }
    }

    /// Records evidence that `peer` was alive at `at`.
    ///
    /// The peer must already be known: evidence with no address to dial is not
    /// something this context can act on, so discovery comes first.
    pub fn record_heartbeat(&mut self, peer: PeerId, at: Millis) -> Result<(), PeerRosterError> {
        self.entry_mut(peer)?.record_evidence(at);
        Ok(())
    }

    /// Opens a session to `peer` in `direction`, resolving a simultaneous
    /// connect if one is already live in the opposite direction.
    ///
    /// A second session in the *same* direction is rejected: the first one is
    /// already a commitment to one link, and a duplicate means the caller lost
    /// track of it.
    ///
    /// An opposite-direction session is the simultaneous-connect case, which is
    /// normal rather than exceptional (invariant 3). The collapse rule decides
    /// which survives; the loser is reported in
    /// [`SessionOutcome::superseded`] for the application to close at the
    /// transport. If the superseded session was already established, the peer
    /// really does stop being reachable for the interval before the survivor
    /// handshakes, so `PeerDisconnected` is published — an honest gap is better
    /// than a `messaging` context that believes a link exists while its bytes
    /// go nowhere.
    ///
    /// Only an **inbound** open is evidence of life: a remote that dialled us
    /// has demonstrably just acted, while our own dial demonstrates nothing
    /// about them.
    pub fn open_session(
        &mut self,
        peer: PeerId,
        direction: SessionDirection,
        at: Millis,
    ) -> Result<SessionOutcome, PeerRosterError> {
        let local = self.local;
        let entry = self.entry_mut(peer)?;

        if matches!(direction, SessionDirection::Inbound) {
            entry.record_evidence(at);
        }

        let incoming = Session::open(local, peer, direction)?;
        let live = entry.session().filter(|session| session.is_live()).cloned();

        let Some(existing) = live else {
            entry.set_session(Some(incoming));
            return Ok(SessionOutcome::quiet());
        };

        if existing.direction() == direction {
            return Err(PeerRosterError::SessionAlreadyOpen);
        }

        let collapse = SessionCollapse::between(local, &existing, &incoming)?;
        let mut outcome = SessionOutcome {
            collapse: Some(collapse),
            superseded: Some(collapse.superseded()),
            ..SessionOutcome::quiet()
        };

        if collapse.survivor() == direction {
            if existing.is_established() {
                outcome.disconnected = Some(PeerDisconnected { peer });
            }
            entry.set_session(Some(incoming));
        }

        Ok(outcome)
    }

    /// Records that the handshake with `peer` completed.
    ///
    /// This is the moment the peer becomes reachable, so it is the only place
    /// `PeerConnected` is produced. `at` is evidence of life: a completed
    /// handshake is proof the remote acted.
    pub fn establish_session(
        &mut self,
        peer: PeerId,
        at: Millis,
    ) -> Result<SessionOutcome, PeerRosterError> {
        let entry = self.entry_mut(peer)?;
        let session = entry.session_mut().ok_or(PeerRosterError::NoSession)?;

        session.establish()?;
        entry.record_evidence(at);

        Ok(SessionOutcome {
            connected: Some(PeerConnected { peer }),
            ..SessionOutcome::quiet()
        })
    }

    /// Ends the session with `peer`, keeping the peer itself known.
    ///
    /// `PeerDisconnected` is published only if the session had established:
    /// no `PeerConnected` was ever published for a session that died while
    /// connecting, and an unmatched disconnect would make `messaging` fail
    /// directs for a peer it never considered reachable (D10).
    ///
    /// Takes no instant: a close is not evidence of life. A locally initiated
    /// close says nothing about the remote at all.
    pub fn close_session(&mut self, peer: PeerId) -> Result<SessionOutcome, PeerRosterError> {
        let entry = self.entry_mut(peer)?;
        let session = entry.session_mut().ok_or(PeerRosterError::NoSession)?;

        let was_established = session.is_established();
        session.close()?;
        entry.set_session(None);

        Ok(SessionOutcome {
            disconnected: was_established.then_some(PeerDisconnected { peer }),
            ..SessionOutcome::quiet()
        })
    }

    /// Re-derives every peer's presence as of `now` and reports those that have
    /// *newly* fallen offline, in `PeerId` order.
    ///
    /// Sessions are untouched. Presence and sessions are orthogonal: silence is
    /// not a close, only the transport can report a dead link, and it is the
    /// application's decision whether an expiry should provoke one.
    ///
    /// A peer is reported once per silence. Fresh evidence re-arms it, so a
    /// peer that returns and goes quiet again expires again.
    pub fn expire_presence(
        &mut self,
        now: Millis,
        windows: LivenessWindows,
    ) -> Vec<PeerPresenceExpired> {
        let mut expired = Vec::new();

        for entry in self.peers.values_mut() {
            let presence = entry.presence(now, windows);
            let newly_offline = presence.is_offline() && !entry.reported_presence().is_offline();

            if newly_offline {
                expired.push(PeerPresenceExpired {
                    peer: entry.peer(),
                    last_evidence_at: entry.last_seen_at(),
                    at: now,
                });
            }
            entry.set_reported_presence(presence);
        }

        expired
    }

    /// Forgets `peer` entirely.
    ///
    /// Reports [`PeerDisconnected`] when the removed entry held an established
    /// session, for the same reason [`close_session`](Self::close_session)
    /// does.
    pub fn remove(&mut self, peer: PeerId) -> Result<Option<PeerDisconnected>, PeerRosterError> {
        self.reject_local(peer)?;
        let entry = self
            .peers
            .remove(&peer)
            .ok_or(PeerRosterError::UnknownPeer)?;

        Ok(entry.is_connected().then_some(PeerDisconnected { peer }))
    }

    fn reject_local(&self, peer: PeerId) -> Result<(), PeerRosterError> {
        if peer == self.local {
            return Err(PeerRosterError::SelfConnection);
        }

        Ok(())
    }

    fn entry_mut(&mut self, peer: PeerId) -> Result<&mut KnownPeer, PeerRosterError> {
        self.reject_local(peer)?;
        self.peers
            .get_mut(&peer)
            .ok_or(PeerRosterError::UnknownPeer)
    }
}

/// Typed rejection of a [`PeerRoster`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRosterError {
    /// The operation named the local peer itself (invariant 2).
    SelfConnection,
    /// The peer has no roster entry; discovery comes first.
    UnknownPeer,
    /// A discovery carried no endpoint, so there is nothing to dial.
    NoEndpoints,
    /// The peer has no session to establish or close.
    NoSession,
    /// A live session in that direction already exists for the peer.
    SessionAlreadyOpen,
    /// The session cannot make the requested transition.
    InvalidSessionTransition {
        from: SessionState,
        to: SessionState,
    },
}

impl From<SessionError> for PeerRosterError {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::SelfConnection => Self::SelfConnection,
            SessionError::InvalidTransition { from, to } => {
                Self::InvalidSessionTransition { from, to }
            }
        }
    }
}

impl From<crate::domain::SessionCollapseError> for PeerRosterError {
    /// A collapse can only fail here in ways the roster has already ruled out —
    /// it holds one session per peer, checks the direction first, and rejects
    /// the local peer at entry — so every variant maps to the one thing that
    /// could still be true: the pair should not have existed.
    fn from(_: crate::domain::SessionCollapseError) -> Self {
        Self::SelfConnection
    }
}

impl fmt::Display for PeerRosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfConnection => f.write_str("the local peer is never a roster entry"),
            Self::UnknownPeer => f.write_str("peer is not in the roster"),
            Self::NoEndpoints => f.write_str("a discovered peer must carry at least one endpoint"),
            Self::NoSession => f.write_str("peer has no live session in the roster"),
            Self::SessionAlreadyOpen => {
                f.write_str("a live session in that direction already exists for the peer")
            }
            Self::InvalidSessionTransition { from, to } => {
                write!(f, "session cannot move from {from} to {to}")
            }
        }
    }
}

impl std::error::Error for PeerRosterError {}
