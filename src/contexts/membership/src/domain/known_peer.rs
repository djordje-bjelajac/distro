use shared_types::PeerId;

use crate::domain::{Endpoint, LivenessWindows, Millis, Presence, Session};

/// One entry of the [`PeerRoster`](crate::domain::PeerRoster): everything this
/// peer locally knows about one remote peer (canvas §2.2).
///
/// Presence is deliberately **not** a field — and now no field here has type
/// [`Presence`] at all, so no constructor can assert one. Storing a presence
/// would make it a fact someone set, and invariant 7 says it is a derivation:
/// [`presence`](Self::presence) computes it on demand from
/// [`last_seen_at`](Self::last_seen_at) and the caller's clock reading.
///
/// # Two instants that must never blur
///
/// * [`last_seen_at`](Self::last_seen_at) is **evidence**: an act the peer
///   itself performed, observed here at approximately the time it happened. It
///   is `Option` because a peer we have only been *told about* has performed
///   none, and that is the state every entry starts in.
/// * [`recorded_at`](Self::recorded_at) is bookkeeping: when this entry came
///   into existence here. It says nothing about the remote peer — a hostile
///   host can cause one at will by publishing a DHT record — so it feeds
///   eviction order and nothing else. It must never reach
///   [`presence`](Self::presence) (canvas D3, S2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeer {
    peer: PeerId,
    endpoints: Vec<Endpoint>,
    last_seen_at: Option<Millis>,
    recorded_at: Millis,
    session: Option<Session>,
    expiry_announced: bool,
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

    /// Records a peer someone reported: addresses, and no evidence whatsoever.
    ///
    /// `recorded_at` is when *we wrote this down*, not when the peer was heard
    /// from — the peer has not been heard from, which is the whole point. A
    /// discovery is a third party's claim (invariant 2), so the entry starts
    /// with `last_seen_at: None` and derives [`Presence::Unknown`] at every
    /// instant until the peer itself acts.
    pub(crate) fn discovered(peer: PeerId, endpoints: Vec<Endpoint>, recorded_at: Millis) -> Self {
        let mut entry = Self {
            peer,
            endpoints: Vec::new(),
            last_seen_at: None,
            recorded_at,
            session: None,
            expiry_announced: false,
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

    /// When this peer last produced evidence of life, or `None` if it never
    /// has.
    pub const fn last_seen_at(&self) -> Option<Millis> {
        self.last_seen_at
    }

    /// Whether this peer has ever produced evidence of life.
    ///
    /// The predicate the peer cache filters on (canvas D8): an identity we were
    /// merely told about must not be written to disk, where the next launch's
    /// first bootstrap rung would dial it.
    pub const fn has_evidence(&self) -> bool {
        self.last_seen_at.is_some()
    }

    /// When this entry was created here.
    ///
    /// Eviction order only (canvas D9). Not evidence, and never an input to
    /// [`presence`](Self::presence).
    pub const fn recorded_at(&self) -> Millis {
        self.recorded_at
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
    ///
    /// `Unknown` while no evidence has arrived, however long the entry has
    /// existed: an entry ages, a peer's silence does not begin until it has
    /// spoken once.
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
    /// Callers are the complete evidence list of invariant 1 — an inbound
    /// session open, a completed handshake, a frame arriving on a link with
    /// that peer, that peer's acknowledgement of a direct request. **Discovery
    /// is not among them** and must never call this: an mDNS sighting, a DHT
    /// record, a gossip announcement, and a cache entry are all things a third
    /// party said (invariant 2, canvas D3).
    ///
    /// The first evidence takes the entry out of [`Presence::Unknown`] whatever
    /// its instant; there is nothing to compare it against.
    ///
    /// Fresh evidence also re-arms the expiry edge: the peer demonstrably
    /// existed at `at`, so a peer that came back and went quiet again must be
    /// able to expire a second time rather than stay silently latched at its
    /// first expiry.
    pub(crate) const fn record_evidence(&mut self, at: Millis) {
        let is_newer = match self.last_seen_at {
            Some(last_seen_at) => at.as_millis() > last_seen_at.as_millis(),
            None => true,
        };

        if is_newer {
            self.last_seen_at = Some(at);
            self.expiry_announced = false;
        }
    }

    /// Whether the last expiry sweep already announced this peer's silence.
    ///
    /// An edge detector, which is all the deleted `reported_presence` field
    /// ever was — it existed to keep `PeerPresenceExpired` from firing twice
    /// for one silence, never to hold a verdict.
    pub(crate) const fn expiry_announced(&self) -> bool {
        self.expiry_announced
    }

    pub(crate) const fn set_expiry_announced(&mut self, announced: bool) {
        self.expiry_announced = announced;
    }

    pub(crate) const fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    pub(crate) fn set_session(&mut self, session: Option<Session>) {
        self.session = session;
    }
}
