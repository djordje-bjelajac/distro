use std::collections::BTreeMap;
use std::sync::Arc;

use membership::domain::events::MembershipEvent;
use membership::domain::{DurationMillis, JoinTicket};
use messaging::domain::{ConversationId, MessageId};
use messaging::ports::{InboundVerdict, MessagePayload};
use shared_types::{PayloadKind, PeerId};

use crate::clock::VirtualClock;
use crate::fabric::{DialFault, DropCause, LinkPolicy, QueuedFrame, SimFabric, SimFrame};
use crate::harness::{DurablePeerState, SimNetworkBuilder, SimPeer, SimSettings};
use crate::trace::{EventTrace, PeerLifecycle, TraceEvent};

/// N peers in one process over one deterministic network: the vehicle every
/// multi-peer claim in the canvas is verified through (S5).
///
/// # The three things a scenario controls, and nothing else
///
/// 1. **Time** — [`advance`](Self::advance). The clock never moves on its own.
/// 2. **Delivery** — [`pump`](Self::pump). The fabric never hands over a frame
///    that was not asked for.
/// 3. **Topology and faults** — partitions, severed links, delays, duplication,
///    signature corruption, LAN segments, relays.
///
/// Everything else follows from the code under test. There is no thread, no
/// timer, no socket, and no real clock anywhere in this crate, so two runs of
/// the same script produce byte-identical traces.
///
/// # The shape of a scenario
///
/// ```ignore
/// let net = SimNetwork::seeded(7).with_peers(["alice", "bob"]).build();
/// let (alice, bob) = (net.peer_id("alice"), net.peer_id("bob"));
///
/// net.boot_all();                                   // identity + join + settle
/// net.named("alice").send_direct(bob, "hello")?;
/// net.settle();
///
/// assert_eq!(net.peer(bob).transcript(ConversationId::Direct(alice)), ["hello"]);
/// ```
///
/// `&mut self` is needed only to add or restart a peer; everything else,
/// including the pump, takes `&self`.
///
/// # Two runtime obligations the composition root owns, made explicit here
///
/// Both contexts state that no timer is started inside them: the presence sweep
/// and the gap sweep are driven from outside, through their inbound ports. The
/// harness does not hide that — [`tick`](Self::tick) is a step a scenario takes,
/// so a scenario that never ticks sees exactly what a root that forgot to would
/// see.
pub struct SimNetwork {
    seed: u64,
    settings: SimSettings,
    clock: Arc<VirtualClock>,
    trace: Arc<EventTrace>,
    fabric: Arc<SimFabric>,
    peers: BTreeMap<PeerId, SimPeer>,
    /// Peers in the order a scenario added them. Every harness-driven sweep
    /// walks this rather than the map, so "alice then bob" means what it says.
    order: Vec<PeerId>,
    labels: BTreeMap<String, PeerId>,
}

impl SimNetwork {
    /// Frames one [`pump`](Self::pump) will deliver before declaring a
    /// livelock.
    ///
    /// A scenario that never settles must fail loudly with a stated cause
    /// rather than hang a test run: a wedged simulation is a defect in the code
    /// under test or in the script, and either is worth a panic naming it.
    pub const MAX_PUMP_STEPS: usize = 100_000;

    /// A builder for a network seeded with `seed`.
    pub const fn seeded(seed: u64) -> SimNetworkBuilder {
        SimNetworkBuilder::seeded(seed)
    }

    pub(crate) fn assemble(seed: u64, epoch: u64, settings: SimSettings) -> Self {
        let clock = Arc::new(VirtualClock::starting_at(epoch));
        let fabric = Arc::new(SimFabric::new(Arc::clone(&clock), seed));

        Self {
            seed,
            settings,
            clock,
            trace: Arc::new(EventTrace::new()),
            fabric,
            peers: BTreeMap::new(),
            order: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    // ---------------------------------------------------------------- peers

    /// Adds a peer named `label`: a fresh identity, an empty cache, online, on
    /// LAN segment 0.
    ///
    /// The peer has assumed nothing and joined nothing — a process that has
    /// been launched and no more, which is what lets a scenario assert on AC1's
    /// first launch rather than being handed the result of it.
    ///
    /// # Panics
    ///
    /// If `label` is already taken. Two peers with one name would make every
    /// later lookup ambiguous.
    pub fn add_peer(&mut self, label: &str) -> PeerId {
        assert!(
            !self.labels.contains_key(label),
            "a peer named {label:?} is already in this network"
        );

        let durable = Arc::new(DurablePeerState::fresh(self.seed, label));
        let peer = durable.peer();

        self.fabric.register(peer, label);
        self.trace.label(peer, label);

        let assembled = SimPeer::assemble(
            label,
            durable,
            &self.fabric,
            &self.clock,
            &self.trace,
            self.settings,
        );

        self.labels.insert(label.to_owned(), peer);
        self.order.push(peer);
        self.peers.insert(peer, assembled);
        self.record(TraceEvent::Lifecycle {
            peer,
            change: PeerLifecycle::Started,
        });

        peer
    }

    /// The identity of the peer named `label`.
    ///
    /// # Panics
    ///
    /// If no peer has that name — a typo in a scenario should fail at the
    /// lookup, not silently address nobody.
    pub fn peer_id(&self, label: &str) -> PeerId {
        *self
            .labels
            .get(label)
            .unwrap_or_else(|| panic!("no peer named {label:?} in this network"))
    }

    /// The name `peer` was added under.
    pub fn label_of(&self, peer: PeerId) -> String {
        self.trace.label_of(peer)
    }

    /// One peer.
    ///
    /// # Panics
    ///
    /// If `peer` is not in this network.
    pub fn peer(&self, peer: PeerId) -> &SimPeer {
        self.peers
            .get(&peer)
            .unwrap_or_else(|| panic!("{} is not in this network", self.label_of(peer)))
    }

    /// One peer, by the name a scenario added it under.
    ///
    /// # Panics
    ///
    /// If no peer has that name.
    pub fn named(&self, label: &str) -> &SimPeer {
        self.peer(self.peer_id(label))
    }

    /// Every peer, in the order they were added.
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.order.clone()
    }

    /// Every peer, in the order they were added.
    pub fn peers(&self) -> impl Iterator<Item = &SimPeer> {
        self.order.iter().map(|peer| self.peer(*peer))
    }

    /// Whether `peer`'s process is running.
    pub fn is_online(&self, peer: PeerId) -> bool {
        self.fabric.is_online(peer)
    }

    /// Starts a stopped peer's process.
    ///
    /// The contexts it had are still the ones it has: this is "the network came
    /// back", not "the process was replaced". For the latter, see
    /// [`restart`](Self::restart).
    pub fn start(&self, peer: PeerId) {
        self.fabric.set_online(peer, true);
        self.record(TraceEvent::Lifecycle {
            peer,
            change: PeerLifecycle::Started,
        });
    }

    /// Kills a peer's process abruptly.
    ///
    /// Nothing is announced and every transport link it held is dropped, so its
    /// neighbours learn of the departure by presence expiry — which is exactly
    /// what AC5 asks be observable. A *graceful* departure is
    /// [`SimPeer::leave`], which closes sessions through the real port and
    /// tells everyone at once.
    pub fn stop(&self, peer: PeerId) {
        self.fabric.set_online(peer, false);
        self.record(TraceEvent::Lifecycle {
            peer,
            change: PeerLifecycle::Stopped,
        });
    }

    /// Replaces a peer's process (D12, AC16).
    ///
    /// Discarded: the three contexts, the roster, every conversation, and the
    /// message log (D7). Kept: the keypair, the peer cache, the trust records,
    /// and the outbound sequence counter — everything whose domain of validity
    /// is the identity rather than the process.
    ///
    /// The rebuilt process assumes its identity, as any launch does (AC1, AC9),
    /// and joins nothing: what a restarted peer does next is the scenario's
    /// decision.
    ///
    /// # Panics
    ///
    /// If `peer` is not in this network.
    pub fn restart(&mut self, peer: PeerId) {
        let (label, durable) = {
            let existing = self.peer(peer);
            (existing.label().to_owned(), Arc::clone(existing.durable()))
        };

        self.fabric.set_online(peer, false);
        self.fabric.reset_peer(peer);

        let rebuilt = SimPeer::assemble(
            &label,
            durable,
            &self.fabric,
            &self.clock,
            &self.trace,
            self.settings,
        );
        self.peers.insert(peer, rebuilt);
        self.fabric.set_online(peer, true);

        self.initialize(peer);
        self.record(TraceEvent::Lifecycle {
            peer,
            change: PeerLifecycle::Restarted,
        });
    }

    /// Assumes `peer`'s identity, as a launch does (AC1, AC9).
    pub fn initialize(&self, peer: PeerId) {
        if let Err(error) = self.peer(peer).initialize_identity() {
            self.refused(peer, "initialize-local-identity", &error);
        }
    }

    /// Brings one peer fully up: it assumes its identity, walks the D1
    /// bootstrap ladder, and the network settles.
    ///
    /// The convenience for scenarios that are not testing the bootstrap itself.
    /// A scenario that *is* should call [`initialize`](Self::initialize) and
    /// [`SimPeer::join`] itself and inspect the [`JoinOutcome`](membership::ports::JoinOutcome).
    pub fn boot(&self, peer: PeerId) {
        self.initialize(peer);

        if let Err(error) = self.peer(peer).join() {
            self.refused(peer, "join-network", &error);
        }

        self.settle();
    }

    /// Boots every peer, in the order they were added, settling after each.
    ///
    /// Sequential rather than simultaneous on purpose: it produces the ordinary
    /// case — each peer discovers the ones already up — rather than the
    /// simultaneous-connect collapse, which is a scenario in its own right
    /// (invariant 3) and should be staged deliberately.
    pub fn boot_all(&self) {
        for peer in &self.order {
            self.boot(*peer);
        }
    }

    // ---------------------------------------------------------------- clock

    /// The virtual instant, in milliseconds.
    pub fn now(&self) -> u64 {
        self.clock.now_millis()
    }

    /// Moves the clock forward. Delivers nothing: frames that become due stay
    /// queued until a pump.
    pub fn advance(&self, millis: u64) {
        self.clock.advance(millis);
    }

    /// The shared clock, for wiring or for a direct reading.
    pub fn clock(&self) -> &Arc<VirtualClock> {
        &self.clock
    }

    // ----------------------------------------------------------------- pump

    /// Delivers every frame due at the current instant, fanning
    /// `PeerConnected` / `PeerDisconnected` into `messaging` between
    /// deliveries, and reports how many frames were handed over.
    ///
    /// Does **not** move the clock, so a frame scheduled into the future stays
    /// in flight. That separation is what makes a scripted delay a real delay
    /// rather than a re-ordering trick.
    ///
    /// # Panics
    ///
    /// If more than [`MAX_PUMP_STEPS`](Self::MAX_PUMP_STEPS) frames are
    /// delivered in one call. A simulation that will not settle is a defect,
    /// and a loud one beats a hung test run.
    pub fn pump(&self) -> usize {
        let mut delivered = 0;
        let mut steps = 0;

        loop {
            let fanned = self.fan_peer_lifecycle();

            match self.fabric.take_due(self.clock.now_millis()) {
                Some(queued) => {
                    self.deliver(queued);
                    delivered += 1;
                }
                None if fanned == 0 => break,
                None => {}
            }

            steps += 1;
            assert!(
                steps <= Self::MAX_PUMP_STEPS,
                "the simulation did not settle after {} pump steps; \
                 a peer is answering every frame with another frame",
                Self::MAX_PUMP_STEPS
            );
        }

        delivered
    }

    /// Delivers at most one due frame, so a scenario can interleave assertions
    /// with individual deliveries.
    ///
    /// Reports whether a frame was handed over.
    pub fn pump_once(&self) -> bool {
        self.fan_peer_lifecycle();

        match self.fabric.take_due(self.clock.now_millis()) {
            Some(queued) => {
                self.deliver(queued);
                true
            }
            None => false,
        }
    }

    /// Runs the network out: pumps, advances the clock to the next queued
    /// frame's due instant, and repeats until nothing is in flight.
    ///
    /// This is "until quiescent". It **does** move the clock — that is how a
    /// delayed frame becomes deliverable — so a scenario asserting that the
    /// clock stands still uses [`pump`](Self::pump) instead.
    ///
    /// # Panics
    ///
    /// If the network will not settle; see [`pump`](Self::pump).
    pub fn settle(&self) -> usize {
        let mut delivered = 0;
        let mut steps = 0;

        loop {
            delivered += self.pump();

            let Some(next) = self.fabric.next_due_at() else {
                break;
            };
            self.clock.advance_to(next);

            steps += 1;
            assert!(
                steps <= Self::MAX_PUMP_STEPS,
                "the network never went quiet after {} settle rounds",
                Self::MAX_PUMP_STEPS
            );
        }

        delivered
    }

    /// Drives both clock-driven sweeps on every running peer — presence
    /// expiry (AC5) and the gap sweep (rule R, AC15) — then pumps.
    ///
    /// Neither context starts a timer for these; the composition root owns them
    /// and so does a scenario. A scenario that never ticks sees exactly what a
    /// root that forgot to would: peers that never go stale, and gaps that only
    /// ever close when a buffer fills.
    pub fn tick(&self) -> usize {
        for peer in &self.order {
            if !self.fabric.is_online(*peer) {
                continue;
            }

            let running = self.peer(*peer);

            if let Err(error) = running.expire_presence() {
                self.refused(*peer, "expire-presence", &error);
            }
            if let Err(error) = running.close_aged_gaps() {
                self.refused(*peer, "close-aged-gaps", &error);
            }
        }

        self.pump()
    }

    /// Puts one heartbeat in flight from every running peer to every peer it
    /// holds a link with.
    ///
    /// Evidence of life is what presence is derived from (invariant 7), so a
    /// scenario that wants peers to stay `Online` beats here, and one that
    /// wants a departure noticed simply stops.
    pub fn heartbeat_tick(&self) {
        for peer in &self.order {
            if !self.fabric.is_online(*peer) {
                continue;
            }

            for other in self.fabric.links_of(*peer) {
                self.fabric.enqueue(*peer, other, SimFrame::Heartbeat);
            }
        }
    }

    /// One round of simulated network life: `millis` pass, everyone beats,
    /// frames are delivered, both sweeps run, and anything they produced is
    /// delivered too.
    ///
    /// The step an AC5 or AC15 scenario repeats. A scenario that wants silence
    /// instead uses [`advance`](Self::advance) and [`tick`](Self::tick).
    pub fn run_for(&self, millis: u64) -> usize {
        self.advance(millis);
        self.heartbeat_tick();

        let delivered = self.pump();
        delivered + self.tick()
    }

    /// Whether nothing is due and nothing is waiting to be fanned out.
    ///
    /// Frames scheduled into the future do not count: the network is quiet
    /// *now*. [`pending_frames`](Self::pending_frames) counts those.
    pub fn is_quiescent(&self) -> bool {
        let nothing_due = self
            .fabric
            .next_due_at()
            .is_none_or(|due| due > self.clock.now_millis());

        nothing_due
            && !self
                .peers
                .values()
                .any(|peer| peer.membership_events().has_pending())
    }

    /// How many frames are in flight, due or not.
    pub fn pending_frames(&self) -> usize {
        self.fabric.pending_frames()
    }

    /// The instant the next queued frame becomes deliverable.
    pub fn next_due_at(&self) -> Option<u64> {
        self.fabric.next_due_at()
    }

    // -------------------------------------------------------------- shaping

    /// The shared fabric, for a topology this harness does not wrap.
    pub fn fabric(&self) -> &Arc<SimFabric> {
        &self.fabric
    }

    /// The latency every link without one of its own has.
    pub fn set_default_delay(&self, millis: u64) {
        self.fabric.set_default_delay(millis);
    }

    /// The latency of one directed link.
    pub fn set_link_delay(&self, from: PeerId, to: PeerId, millis: u64) {
        self.fabric.set_link_delay(from, to, millis);
    }

    /// Replaces one directed link's whole policy.
    pub fn set_link_policy(&self, from: PeerId, to: PeerId, policy: LinkPolicy) {
        self.fabric.set_link_policy(from, to, policy);
    }

    /// Queues per-message delays, consumed one per message frame in the order
    /// they are handed to the transport (AC8, AC10).
    ///
    /// This is how a scenario writes a delivery order down. Session,
    /// acknowledgement, and heartbeat frames do not consume the script.
    pub fn script_delays(&self, delays: impl IntoIterator<Item = u64>) {
        self.fabric.script_delays(delays);
    }

    /// Makes the next `count` message frames arrive twice (AC7).
    pub fn duplicate_next(&self, count: usize) {
        self.fabric.duplicate_next(count);
    }

    /// Flips a signature bit on the next `count` message frames (AC6).
    pub fn corrupt_next_signatures(&self, count: usize) {
        self.fabric.corrupt_next_signatures(count);
    }

    /// Cuts the link between two peers in both directions.
    ///
    /// The rest of the network is intact, so a third peer that can reach both
    /// ends may still relay around it — which is AC12.
    pub fn sever_link(&self, a: PeerId, b: PeerId) {
        self.fabric.sever_link(a, b);
    }

    /// Cuts one direction only.
    pub fn sever_link_one_way(&self, from: PeerId, to: PeerId) {
        self.fabric.sever_link_one_way(from, to);
    }

    /// Restores a severed link in both directions.
    pub fn restore_link(&self, a: PeerId, b: PeerId) {
        self.fabric.restore_link(a, b);
    }

    /// Makes dials along one directed link fail in the stated way.
    pub fn set_dial_fault(&self, from: PeerId, to: PeerId, fault: DialFault) {
        self.fabric.set_dial_fault(from, to, fault);
    }

    /// Splits the network: `group` exchanges nothing with any other group, and
    /// no relay bridges the split (AC5).
    pub fn set_partition_group(&self, peer: PeerId, group: u32) {
        self.fabric.set_partition_group(peer, group);
    }

    /// Moves the listed peers onto their own side of a split, leaving everyone
    /// else where they are.
    ///
    /// # Panics
    ///
    /// If `peers` is empty — a partition with nothing on one side is a
    /// scenario bug rather than a topology.
    pub fn partition_off(&self, peers: &[PeerId]) {
        assert!(
            !peers.is_empty(),
            "a partition needs at least one peer on the far side"
        );

        let group = self
            .order
            .iter()
            .map(|peer| self.fabric.partition_group(*peer))
            .max()
            .unwrap_or(0)
            + 1;

        for peer in peers {
            self.fabric.set_partition_group(*peer, group);
        }
    }

    /// Puts every peer back on one network.
    pub fn heal_partitions(&self) {
        self.fabric.heal_partitions();
    }

    /// Moves `peer` onto a broadcast domain; discovery reaches only the same
    /// segment (AC2, AC3).
    pub fn set_lan_segment(&self, peer: PeerId, segment: u32) {
        self.fabric.set_lan_segment(peer, segment);
    }

    /// Puts `peer` on a LAN of its own, so no neighbour can discover it and the
    /// bootstrap ladder must fall through to a ticket (AC3, D1).
    pub fn isolate_from_lan(&self, peer: PeerId) {
        let segment = self
            .order
            .iter()
            .map(|other| self.fabric.lan_segment(*other))
            .max()
            .unwrap_or(0)
            + 1;

        self.fabric.set_lan_segment(peer, segment);
    }

    /// Publishes a relayed address for `target` through `via` (AC12).
    ///
    /// Discovery then hands the relayed address out alongside the direct one,
    /// so a dialer that cannot reach `target` directly still connects, and the
    /// endpoint that answered says a third peer is carrying the traffic.
    pub fn advertise_relay(&self, target: PeerId, via: PeerId) {
        self.fabric.advertise_relay(target, via);
    }

    /// Whether `peer` offers relay service to others (AC4 says every instance
    /// does; this takes it away).
    pub fn set_relay_capable(&self, peer: PeerId, capable: bool) {
        self.fabric.set_relay_capable(peer, capable);
    }

    /// Whether the fabric routes direct messages around severed links at all.
    ///
    /// Turning it off is how a scenario stages S7's stated limit: with no relay
    /// available, two unreachable peers simply cannot exchange messages, and
    /// the send fails with `NoRelayAvailable`.
    pub fn set_relaying_enabled(&self, enabled: bool) {
        self.fabric.set_relaying_enabled(enabled);
    }

    /// Whether a 1:1 message requires a transport link to the recipient (D4).
    pub fn set_direct_requires_session(&self, required: bool) {
        self.fabric.set_direct_requires_session(required);
    }

    /// Whether `peer`'s transport accepts inbound links (AC3's "not listening"
    /// diagnostic).
    pub fn set_can_listen(&self, peer: PeerId, can_listen: bool) {
        self.fabric.set_can_listen(peer, can_listen);
    }

    /// Whether `peer`'s announcements are refused (AC3's "not announced"
    /// diagnostic).
    pub fn set_announce_refused(&self, peer: PeerId, refused: bool) {
        self.fabric.set_announce_refused(peer, refused);
    }

    /// Re-announces every running peer, so a peer already discovered is
    /// reported again by the next `observe_peers`.
    pub fn mdns_tick(&self) {
        self.fabric.mdns_tick();
    }

    // -------------------------------------------------------------- tickets

    /// Mints a join ticket for `issuer` at this peer's current addresses, valid
    /// for the default lifetime (D1).
    ///
    /// # Panics
    ///
    /// If `issuer` is not in this network.
    pub fn join_ticket_from(&self, issuer: PeerId) -> JoinTicket {
        self.join_ticket_expiring_after(issuer, JoinTicket::DEFAULT_LIFETIME)
    }

    /// Mints a join ticket valid for `lifetime` from now.
    ///
    /// A lifetime of zero produces an already-expired ticket, which is how a
    /// scenario pins the "expired" half of the ticket rung's diagnostic.
    ///
    /// # Panics
    ///
    /// If `issuer` is not in this network.
    pub fn join_ticket_expiring_after(
        &self,
        issuer: PeerId,
        lifetime: DurationMillis,
    ) -> JoinTicket {
        let endpoints = self.fabric.endpoints_of(issuer);
        assert!(
            !endpoints.is_empty(),
            "{} has no endpoint to put in a ticket",
            self.label_of(issuer)
        );

        JoinTicket::expiring_after(
            issuer,
            endpoints,
            self.settings.protocol,
            self.clock.membership_now(),
            lifetime,
        )
        .expect("a registered peer always has at least one endpoint")
    }

    // ---------------------------------------------------------------- trace

    /// The ordered record of everything that has happened.
    pub fn trace(&self) -> &Arc<EventTrace> {
        &self.trace
    }

    /// The trace as text — what two runs are compared on.
    pub fn render_trace(&self) -> String {
        self.trace.render()
    }

    /// Discards the trace so far, keeping peer labels.
    pub fn clear_trace(&self) {
        self.trace.clear();
    }

    // ----------------------------------------------------------- randomness

    /// The seed this network was built with.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// A seeded draw below `bound`, for any choice a scenario itself makes.
    pub fn random_below(&self, bound: u64) -> u64 {
        self.fabric.random_below(bound)
    }

    /// Shuffles `items` from the seeded stream.
    pub fn shuffle<T>(&self, items: &mut [T]) {
        self.fabric.shuffle(items);
    }

    /// The settings every peer in this network was assembled with.
    pub const fn settings(&self) -> SimSettings {
        self.settings
    }

    // -------------------------------------------------------------- internal

    fn record(&self, event: TraceEvent) {
        self.trace.record(self.clock.now_millis(), event);
    }

    fn refused(&self, peer: PeerId, operation: &'static str, error: &dyn std::fmt::Display) {
        self.record(TraceEvent::PortRefused {
            peer,
            operation,
            reason: error.to_string(),
        });
    }

    /// Hands `membership`'s two cross-context events to `messaging` (D10).
    ///
    /// This is the composition root's subscription, made a visible step. Doing
    /// it inside the publisher would run one context's command inside another's
    /// and in an order no scenario could predict; doing it here keeps it
    /// ordered, drainable, and traceable.
    fn fan_peer_lifecycle(&self) -> usize {
        let mut fanned = 0;

        for peer in &self.order {
            let running = self.peer(*peer);

            for event in running.membership_events().drain_cross_context() {
                fanned += 1;

                match event {
                    MembershipEvent::PeerConnected(connected) => {
                        if let Err(error) = running.peer_connected(connected) {
                            self.refused(*peer, "peer-connected", &error);
                        }
                    }
                    MembershipEvent::PeerDisconnected(disconnected) => {
                        if let Err(error) = running.peer_disconnected(disconnected) {
                            self.refused(*peer, "peer-disconnected", &error);
                        }
                    }
                    // `is_cross_context` already excluded the rest; leaking a
                    // context-internal event would hand `messaging` endpoints
                    // and sessions it must never see.
                    _ => {}
                }
            }
        }

        fanned
    }

    /// Hands one frame to its destination's inbound ports, or records why it
    /// could not be.
    fn deliver(&self, queued: QueuedFrame) {
        let QueuedFrame {
            from, to, frame, ..
        } = queued;
        let label = frame.label();

        if let Some(cause) = self.fabric.delivery_block(from, to) {
            self.record(TraceEvent::FrameDropped {
                from,
                to,
                frame: label,
                cause,
            });
            return;
        }

        let Some(target) = self.peers.get(&to) else {
            self.record(TraceEvent::FrameDropped {
                from,
                to,
                frame: label,
                cause: DropCause::DestinationUnknown,
            });
            return;
        };

        self.record(TraceEvent::FrameDelivered {
            from,
            to,
            frame: label,
        });

        let refusal = match frame {
            SimFrame::SessionOpened { endpoints } => target
                .session_opened(from, endpoints)
                .err()
                .map(|error| error.to_string()),
            SimFrame::SessionEstablished => target
                .session_established(from)
                .err()
                .map(|error| error.to_string()),
            SimFrame::SessionClosed => target
                .session_closed(from)
                .err()
                .map(|error| error.to_string()),
            SimFrame::Heartbeat => target
                .peer_heartbeat(from)
                .err()
                .map(|error| error.to_string()),
            SimFrame::Acknowledgement(id) => target
                .message_delivered(id)
                .err()
                .map(|error| error.to_string()),
            SimFrame::Message(envelope) => self.deliver_message(target, from, envelope),
        };

        if let Some(reason) = refusal {
            self.record(TraceEvent::FrameRefused {
                from,
                to,
                frame: label,
                reason,
            });
        }
    }

    /// Takes one envelope in at a peer's inbound boundary and, when the
    /// recipient accepted a 1:1 message, sends the acknowledgement back
    /// (AC11).
    fn deliver_message(
        &self,
        target: &SimPeer,
        from: PeerId,
        envelope: shared_types::Envelope,
    ) -> Option<String> {
        // Traffic is evidence of life. A peer this one has never heard of has
        // no roster entry to refresh, which is not a refusal worth reporting.
        let _ = target.peer_heartbeat(from);

        let kind = envelope.kind;
        let sequence = MessagePayload::decode(&envelope.payload)
            .ok()
            .map(|payload| payload.sequence());

        match target.accept_envelope(envelope) {
            Err(error) => Some(error.to_string()),
            Ok(verdict) => {
                if self.settings.acknowledge_directs
                    && kind == PayloadKind::DirectMessage
                    && acknowledgeable(&verdict)
                    && let Some(sequence) = sequence
                {
                    // The identifier as the *sender* knows it: the recipient's
                    // `Direct(sender)` message is the sender's
                    // `Direct(recipient)` message, and an acknowledgement in the
                    // wrong conversation names nothing.
                    let id = MessageId::new(from, ConversationId::Direct(target.id()), sequence);
                    self.fabric
                        .enqueue(target.id(), from, SimFrame::Acknowledgement(id));
                }

                verdict.rejection_reason().map(|reason| reason.to_string())
            }
        }
    }
}

/// Whether an inbound verdict means the recipient has the message.
///
/// Applied, held behind a gap, and already-seen all mean it arrived; refused at
/// the boundary and rejected by the conversation mean it did not, and
/// acknowledging those would tell a sender its message landed when nothing
/// reached a read model (invariant 10).
fn acknowledgeable(verdict: &InboundVerdict) -> bool {
    verdict.is_applied() || verdict.is_buffered() || verdict.is_duplicate()
}
