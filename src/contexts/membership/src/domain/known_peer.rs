use shared_types::PeerId;

use crate::domain::{Endpoint, LivenessWindows, Millis, Presence, Session};

/// One entry of the [`PeerRoster`](crate::domain::PeerRoster): everything this
/// peer locally knows about one remote peer (canvas §2.2).
///
/// Presence is deliberately **not** a field. Storing it would make it a fact
/// someone set, and invariant 7 says it is a derivation — so
/// [`presence`](Self::presence) computes it on demand from
/// [`last_seen_at`](Self::last_seen_at) and the caller's clock reading. The
/// only presence-shaped state here is `reported_presence`, which is private and
/// means "what the last expiry sweep announced", used solely to keep
/// `PeerPresenceExpired` from firing twice for one silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeer {
    peer: PeerId,
    endpoints: Vec<Endpoint>,
    last_seen_at: Millis,
    session: Option<Session>,
    reported_presence: Presence,
}

impl KnownPeer {
    /// Most addresses retained for one peer.
    ///
    /// A peer legitimately has a handful — a LAN address, a public v4 and v6,
    /// one or two relay circuits — so eight is generous. The cap exists
    /// because endpoints arrive from the network: without it, one peer could
    /// announce thousands of addresses and grow every roster and every peer
    /// cache file in the network at no cost to itself.
    pub const MAX_ENDPOINTS: usize = 8;

    pub(crate) fn discovered(peer: PeerId, endpoints: Vec<Endpoint>, at: Millis) -> Self {
        let mut entry = Self {
            peer,
            endpoints: Vec::new(),
            last_seen_at: at,
            session: None,
            reported_presence: Presence::Online,
        };
        entry.merge_endpoints(endpoints);
        entry
    }

    pub const fn peer(&self) -> PeerId {
        self.peer
    }

    /// Known addresses, oldest first.
    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    /// When this peer last produced evidence of life.
    pub const fn last_seen_at(&self) -> Millis {
        self.last_seen_at
    }

    /// The live or closed session held for this peer, if any.
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Whether an established session exists — the only sense in which this
    /// context calls a peer "connected".
    pub fn is_connected(&self) -> bool {
        self.session.as_ref().is_some_and(Session::is_established)
    }

    /// Derives this peer's presence as of `now` (invariant 7).
    pub fn presence(&self, now: Millis, windows: LivenessWindows) -> Presence {
        Presence::derive(self.last_seen_at, now, windows)
    }

    /// Adds addresses not already held, then enforces
    /// [`MAX_ENDPOINTS`](Self::MAX_ENDPOINTS) by dropping the oldest.
    ///
    /// Newest-wins is the right end to keep: an address a peer just announced
    /// is the one most likely still reachable, while the address it announced
    /// first is the likeliest to be a stale lease.
    pub(crate) fn merge_endpoints(&mut self, endpoints: Vec<Endpoint>) {
        for endpoint in endpoints {
            if !self.endpoints.contains(&endpoint) {
                self.endpoints.push(endpoint);
            }
        }

        let excess = self.endpoints.len().saturating_sub(Self::MAX_ENDPOINTS);
        if excess > 0 {
            self.endpoints.drain(..excess);
        }
    }

    /// Records evidence of life, never moving the instant backwards.
    ///
    /// Fresh evidence also re-arms the expiry edge: the peer demonstrably
    /// existed at `at`, so a peer that came back and went quiet again must be
    /// able to expire a second time rather than stay silently latched at its
    /// first expiry.
    pub(crate) const fn record_evidence(&mut self, at: Millis) {
        if at.as_millis() > self.last_seen_at.as_millis() {
            self.last_seen_at = at;
            self.reported_presence = Presence::Online;
        }
    }

    pub(crate) const fn reported_presence(&self) -> Presence {
        self.reported_presence
    }

    pub(crate) const fn set_reported_presence(&mut self, presence: Presence) {
        self.reported_presence = presence;
    }

    pub(crate) const fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    pub(crate) fn set_session(&mut self, session: Option<Session>) {
        self.session = session;
    }
}
