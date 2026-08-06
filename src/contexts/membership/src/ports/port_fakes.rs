//! Deterministic fakes for this context's outbound ports.
//!
//! Test-only (`#[cfg(test)]`) and never linked into a binary. Domain and
//! application tests must touch no network, clock, filesystem, or external
//! service (AC13), so every collaborator those tests need is implemented here
//! in memory, with no threads, no randomness, and no I/O.
//!
//! Interior mutability rather than `&mut self` is deliberate: every port takes
//! `&self`, so a fake that recorded its calls through a mutable borrow would
//! not implement the trait a real adapter must. It is `Mutex`/atomics rather
//! than `Cell`/`RefCell` because the application layer holds its ports as
//! `Arc<dyn …Port + Send + Sync>` — the shape a composition root needs. The
//! locking is uncontended in tests and never a source of nondeterminism.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use shared_types::PeerId;

use crate::domain::events::MembershipEvent;
use crate::domain::{DurationMillis, Endpoint, JoinTicket, Millis, NetworkStatus};
use crate::ports::{
    CachedPeer, ClockPort, DiscoveredPeer, EventPublisherError, EventPublisherPort, PeerCacheError,
    PeerCachePort, PeerDiscoveryError, PeerDiscoveryPort, PeerTransportError, PeerTransportPort,
};

/// Reads a fake's lock without panicking on a poisoned mutex: a fake that
/// failed an assertion in one test must not turn every later test into a panic
/// with a misleading cause.
fn guard<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A clock that only moves when a test moves it.
pub(crate) struct ManualClock {
    now: Mutex<Millis>,
}

impl ManualClock {
    pub(crate) const fn starting_at(now: Millis) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    /// Moves time forward. Never backwards — the port's contract is monotonic.
    pub(crate) fn advance(&self, span: DurationMillis) {
        let mut now = guard(&self.now);
        *now = now.saturating_add(span);
    }
}

impl ClockPort for ManualClock {
    fn now(&self) -> Millis {
        *guard(&self.now)
    }
}

/// A clock that counts its readings and moves on every one of them.
///
/// Two properties a [`ManualClock`] cannot express:
///
/// * **How many times a handler asked.** [`readings`](Self::readings) is the
///   direct proof that a query took one instant rather than one per peer or one
///   per half of an answer.
/// * **Whether it mattered.** Each reading is `step` later than the last, so a
///   handler that read twice derives two different ages from the same evidence.
///   With a `step` that crosses a liveness window, the second reading lands on
///   the other side of a boundary and the resulting view is visibly
///   inconsistent — the counter says a defect exists, this says what it would
///   cost.
///
/// The first reading is the starting instant, so a correct single-reading caller
/// sees exactly the same view a `ManualClock` frozen there would produce.
/// Monotonic, as [`ClockPort`]'s contract requires.
pub(crate) struct TickingClock {
    next: Mutex<Millis>,
    step: DurationMillis,
    readings: AtomicUsize,
}

impl TickingClock {
    pub(crate) const fn from(start: Millis, step: DurationMillis) -> Self {
        Self {
            next: Mutex::new(start),
            step,
            readings: AtomicUsize::new(0),
        }
    }

    /// How often [`ClockPort::now`] has been called.
    pub(crate) fn readings(&self) -> usize {
        self.readings.load(Ordering::Relaxed)
    }
}

impl ClockPort for TickingClock {
    fn now(&self) -> Millis {
        self.readings.fetch_add(1, Ordering::Relaxed);

        let mut next = guard(&self.next);
        let reading = *next;
        *next = next.saturating_add(self.step);
        reading
    }
}

/// Samples the network status every time a port it is attached to is called,
/// so a test can see what a caller *would* have observed while a handler was
/// still running.
///
/// This is how `NetworkStatus::Joining` is asserted without a thread: the
/// bootstrap ladder is synchronous, so the only vantage point inside it is a
/// port it calls. The probe is a bare `Fn() -> NetworkStatus` closure supplied
/// by the test, which keeps the dependency pointing the right way — a fake in
/// `ports` names a domain type and never the application layer that produces
/// it.
pub(crate) struct StatusProbe {
    probe: Box<dyn Fn() -> NetworkStatus + Send + Sync>,
    observed: Mutex<Vec<NetworkStatus>>,
}

impl StatusProbe {
    pub(crate) fn watching(probe: impl Fn() -> NetworkStatus + Send + Sync + 'static) -> Self {
        Self {
            probe: Box::new(probe),
            observed: Mutex::new(Vec::new()),
        }
    }

    fn sample(&self) {
        let status = (self.probe)();
        guard(&self.observed).push(status);
    }

    /// Every status sampled, in call order.
    pub(crate) fn observed(&self) -> Vec<NetworkStatus> {
        guard(&self.observed).clone()
    }
}

/// A discovery mechanism with a scripted view of the network.
///
/// [`observe_peers`](PeerDiscoveryPort::observe_peers) is **non-draining**, and
/// deliberately so: the port's contract is that reading is a question rather
/// than a withdrawal (canvas `0010` D12, A7), and a fake that emptied itself
/// would let a handler which joins twice pass here while failing in the field —
/// exactly the defect that was observed. `observable` is therefore immutable
/// for the fake's whole life and every call clones it.
pub(crate) struct ScriptedDiscovery {
    observable: Vec<DiscoveredPeer>,
    observation_failure: Option<PeerDiscoveryError>,
    redeemable: Option<DiscoveredPeer>,
    announcements: Mutex<Vec<Vec<Endpoint>>>,
    redemptions: AtomicUsize,
    probe: Option<Arc<StatusProbe>>,
}

impl ScriptedDiscovery {
    pub(crate) const fn observing(observable: Vec<DiscoveredPeer>) -> Self {
        Self {
            observable,
            observation_failure: None,
            redeemable: None,
            announcements: Mutex::new(Vec::new()),
            redemptions: AtomicUsize::new(0),
            probe: None,
        }
    }

    /// Makes ticket redemption succeed with `peer`.
    pub(crate) fn with_redeemable(mut self, peer: DiscoveredPeer) -> Self {
        self.redeemable = Some(peer);
        self
    }

    /// Makes LAN observation fail while leaving ticket redemption alone — the
    /// combination that forces the ladder down to its last rung.
    pub(crate) const fn with_observation_failure(mut self, error: PeerDiscoveryError) -> Self {
        self.observation_failure = Some(error);
        self
    }

    /// Samples the network status on every observation and redemption.
    pub(crate) fn with_status_probe(mut self, probe: Arc<StatusProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Every endpoint set handed to [`PeerDiscoveryPort::announce`], in order.
    pub(crate) fn announcements(&self) -> Vec<Vec<Endpoint>> {
        guard(&self.announcements).clone()
    }

    /// How often a join ticket was redeemed.
    pub(crate) fn redemptions(&self) -> usize {
        self.redemptions.load(Ordering::Relaxed)
    }

    fn sample_status(&self) {
        if let Some(probe) = &self.probe {
            probe.sample();
        }
    }
}

impl PeerDiscoveryPort for ScriptedDiscovery {
    fn announce(&self, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        guard(&self.announcements).push(endpoints.to_vec());
        Ok(())
    }

    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        self.sample_status();
        match self.observation_failure {
            Some(error) => Err(error),
            None => Ok(self.observable.clone()),
        }
    }

    fn redeem_join_ticket(
        &self,
        _ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        self.sample_status();
        self.redemptions.fetch_add(1, Ordering::Relaxed);
        self.redeemable
            .clone()
            .ok_or(PeerDiscoveryError::TicketUnreachable)
    }
}

/// A discovery mechanism that is not running.
pub(crate) struct UnavailableDiscovery;

impl PeerDiscoveryPort for UnavailableDiscovery {
    fn announce(&self, _endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        Err(PeerDiscoveryError::Unavailable)
    }

    fn observe_peers(&self) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        Err(PeerDiscoveryError::Unavailable)
    }

    fn redeem_join_ticket(
        &self,
        _ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        Err(PeerDiscoveryError::Unavailable)
    }
}

/// A transport whose reachable endpoints are scripted.
pub(crate) struct ScriptedTransport {
    listen_endpoints: Vec<Endpoint>,
    reachable: Vec<Endpoint>,
    dialled: Mutex<Vec<PeerId>>,
    open: Mutex<Vec<PeerId>>,
    closed: Mutex<Vec<PeerId>>,
}

impl ScriptedTransport {
    pub(crate) const fn listening_on(listen_endpoints: Vec<Endpoint>) -> Self {
        Self {
            listen_endpoints,
            reachable: Vec::new(),
            dialled: Mutex::new(Vec::new()),
            open: Mutex::new(Vec::new()),
            closed: Mutex::new(Vec::new()),
        }
    }

    /// Declares one endpoint that answers a dial.
    pub(crate) fn reachable_at(mut self, endpoint: Endpoint) -> Self {
        self.reachable.push(endpoint);
        self
    }

    /// Peers a dial was attempted against, in order, whether or not it worked.
    pub(crate) fn dialled(&self) -> Vec<PeerId> {
        guard(&self.dialled).clone()
    }

    /// Peers whose sessions were closed, in order.
    pub(crate) fn closed(&self) -> Vec<PeerId> {
        guard(&self.closed).clone()
    }
}

impl PeerTransportPort for ScriptedTransport {
    fn listen(&self) -> Result<Vec<Endpoint>, PeerTransportError> {
        Ok(self.listen_endpoints.clone())
    }

    fn dial(&self, peer: PeerId, endpoints: &[Endpoint]) -> Result<Endpoint, PeerTransportError> {
        guard(&self.dialled).push(peer);

        let answered = endpoints
            .iter()
            .find(|endpoint| self.reachable.contains(endpoint))
            .cloned()
            .ok_or(PeerTransportError::NoReachableEndpoint)?;

        guard(&self.open).push(peer);
        Ok(answered)
    }

    fn close_session(&self, peer: PeerId) -> Result<(), PeerTransportError> {
        if !guard(&self.open).contains(&peer) {
            return Err(PeerTransportError::NoSuchSession);
        }

        guard(&self.closed).push(peer);
        Ok(())
    }
}

/// A transport that fails every operation with one typed error.
pub(crate) struct UnusableTransport(pub(crate) PeerTransportError);

impl PeerTransportPort for UnusableTransport {
    fn listen(&self) -> Result<Vec<Endpoint>, PeerTransportError> {
        Err(self.0)
    }

    fn dial(&self, _peer: PeerId, _endpoints: &[Endpoint]) -> Result<Endpoint, PeerTransportError> {
        Err(self.0)
    }

    fn close_session(&self, _peer: PeerId) -> Result<(), PeerTransportError> {
        Err(self.0)
    }
}

/// A peer cache held in memory for the length of a test.
pub(crate) struct InMemoryPeerCache {
    peers: Mutex<Vec<CachedPeer>>,
    saves: AtomicUsize,
    /// One entry per write, holding what that write contained.
    ///
    /// The final contents are not enough to test forgetting: a handler that
    /// writes an empty set and *then* a populated one ends up with the wrong
    /// file, and a handler that writes them the other way round ends up with
    /// the right one — from the same two writes. Only the order tells them
    /// apart, and the order is the operation.
    history: Mutex<Vec<Vec<PeerId>>>,
    probe: Option<Arc<StatusProbe>>,
}

impl InMemoryPeerCache {
    pub(crate) const fn empty() -> Self {
        Self {
            peers: Mutex::new(Vec::new()),
            saves: AtomicUsize::new(0),
            history: Mutex::new(Vec::new()),
            probe: None,
        }
    }

    pub(crate) const fn holding(peers: Vec<CachedPeer>) -> Self {
        Self {
            peers: Mutex::new(peers),
            saves: AtomicUsize::new(0),
            history: Mutex::new(Vec::new()),
            probe: None,
        }
    }

    /// Every write so far, in order, as the peer ids it carried.
    pub(crate) fn write_history(&self) -> Vec<Vec<PeerId>> {
        guard(&self.history).clone()
    }

    /// Samples the network status on every load — the first rung of the
    /// bootstrap ladder, and so the earliest vantage point inside a join.
    pub(crate) fn with_status_probe(mut self, probe: Arc<StatusProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// How often the cache was written.
    pub(crate) fn saves(&self) -> usize {
        self.saves.load(Ordering::Relaxed)
    }
}

impl PeerCachePort for InMemoryPeerCache {
    fn load(&self) -> Result<Vec<CachedPeer>, PeerCacheError> {
        if let Some(probe) = &self.probe {
            probe.sample();
        }

        Ok(guard(&self.peers).clone())
    }

    fn save(&self, peers: &[CachedPeer]) -> Result<(), PeerCacheError> {
        *guard(&self.peers) = peers.to_vec();
        guard(&self.history).push(peers.iter().map(|entry| entry.peer).collect());
        self.saves.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A peer cache that fails every operation with one typed error.
pub(crate) struct UnusablePeerCache(pub(crate) PeerCacheError);

impl PeerCachePort for UnusablePeerCache {
    fn load(&self) -> Result<Vec<CachedPeer>, PeerCacheError> {
        Err(self.0)
    }

    fn save(&self, _peers: &[CachedPeer]) -> Result<(), PeerCacheError> {
        Err(self.0)
    }
}

/// A publisher that keeps every event in the order it received them.
pub(crate) struct RecordingPublisher {
    published: Mutex<Vec<MembershipEvent>>,
}

impl RecordingPublisher {
    pub(crate) const fn new() -> Self {
        Self {
            published: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn published(&self) -> Vec<MembershipEvent> {
        guard(&self.published).clone()
    }

    /// Only the events that leave this context — what `messaging` and any
    /// other consumer actually see (canvas §4).
    pub(crate) fn cross_context(&self) -> Vec<MembershipEvent> {
        guard(&self.published)
            .iter()
            .filter(|event| event.is_cross_context())
            .copied()
            .collect()
    }
}

impl EventPublisherPort for RecordingPublisher {
    fn publish(&self, event: MembershipEvent) -> Result<(), EventPublisherError> {
        guard(&self.published).push(event);
        Ok(())
    }
}

/// A publisher that always fails with one typed error.
pub(crate) struct FailingPublisher(pub(crate) EventPublisherError);

impl EventPublisherPort for FailingPublisher {
    fn publish(&self, _event: MembershipEvent) -> Result<(), EventPublisherError> {
        Err(self.0)
    }
}
