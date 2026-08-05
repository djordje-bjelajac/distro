use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use membership::domain::{Endpoint, JoinTicket, Reachability};
use membership::ports::{DiscoveredPeer, PeerDiscoveryError, PeerTransportError};
use messaging::ports::MessageTransportError;
use shared_types::{Envelope, EnvelopeSignature, PeerId};

use crate::clock::VirtualClock;
use crate::fabric::{DialFault, DropCause, LinkPolicy, QueuedFrame, SimFrame};
use crate::rng::SeededRng;
use crate::stores::guard;

/// The in-process network every simulated peer's transports and discovery run
/// over.
///
/// # What it is
///
/// One shared routing core: a directory of who is where, a policy per directed
/// link, and a queue of frames in flight. It has no sockets, no threads, and no
/// timers. Nothing moves without an explicit pump from the harness, and nothing
/// becomes deliverable without an explicit clock advance — which is what makes
/// "the fabric delivers nothing on its own" a property a test can assert rather
/// than a convention to be careful about.
///
/// # The three ports it serves, and why it serves all three
///
/// `PeerTransportPort` (dial, listen, close), `PeerDiscoveryPort` (announce,
/// observe, redeem), and `MessageTransportPort` (send, publish) are three
/// contexts' worth of contract, but they are one *network*: a message can only
/// reach a peer a dial could reach, and a peer can only be dialled at an
/// address discovery published. Splitting the fabric per port would let those
/// three views of one network drift, and a scenario would be asserting against
/// a topology that does not exist.
///
/// The three thin adapters that hold a `PeerId` and delegate here keep each
/// context's port surface exactly as narrow as its trait —
/// `messaging` still never learns what an `Endpoint` is.
///
/// # Two kinds of network failure, deliberately distinct
///
/// * **Partition** ([`set_partition_group`](Self::set_partition_group)) splits
///   the network. Peers in different groups exchange nothing, and no relay can
///   bridge them, because a relay on the far side is unreachable too. This is
///   the AC5 condition.
/// * **Severed link** ([`sever_link`](Self::sever_link)) breaks one path
///   through an otherwise intact network. A third peer that can reach both ends
///   *can* relay around it — which is exactly AC12, and the reason the two are
///   not one knob.
///
/// # Determinism
///
/// Delivery order is `(due_at, enqueue_id)`, both integers, and every
/// collection iterated to build a delivery set is a `BTree*`. The only source
/// of chance is [`SeededRng`], seeded once per network. Two runs of the same
/// script therefore deliver the same frames in the same order at the same
/// virtual instants.
pub struct SimFabric {
    clock: Arc<VirtualClock>,
    state: Mutex<FabricState>,
}

struct FabricState {
    rng: SeededRng,
    peers: BTreeMap<PeerId, PeerSlot>,
    addresses: BTreeMap<String, EndpointRoute>,
    links: BTreeMap<(PeerId, PeerId), LinkPolicy>,
    default_policy: LinkPolicy,
    scripted_delays: VecDeque<u64>,
    duplicate_budget: usize,
    corrupt_budget: usize,
    queue: Vec<QueuedFrame>,
    next_frame_id: u64,
    relaying_enabled: bool,
    direct_requires_session: bool,
}

struct PeerSlot {
    label: String,
    online: bool,
    can_listen: bool,
    announce_refused: bool,
    relay_capable: bool,
    /// Which broadcast domain this peer's discovery mechanism can see (AC2).
    lan_segment: u32,
    /// Which side of a network split this peer is on (AC5).
    partition_group: u32,
    /// Bumped by every announcement; an observer reports a peer once per
    /// generation, which is how "peers seen since the last call" stays true
    /// without a re-announcement being invisible.
    announce_generation: u64,
    announced_endpoints: Vec<Endpoint>,
    observed: BTreeMap<PeerId, u64>,
    sessions: BTreeSet<PeerId>,
    /// Peers through which a relayed endpoint is advertised for this one
    /// (AC12).
    relays: BTreeSet<PeerId>,
}

/// What one published address resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointRoute {
    Direct(PeerId),
    Relayed { target: PeerId, via: PeerId },
}

/// What a dial along one path produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialVerdict {
    Answered,
    /// Nothing answered.
    Silent,
    /// Something answered and the authenticated handshake did not complete.
    HandshakeFailed,
}

impl SimFabric {
    /// A fabric with no peers, reading `clock` and drawing from `seed`.
    pub fn new(clock: Arc<VirtualClock>, seed: u64) -> Self {
        Self {
            clock,
            state: Mutex::new(FabricState {
                rng: SeededRng::from_seed(seed),
                peers: BTreeMap::new(),
                addresses: BTreeMap::new(),
                links: BTreeMap::new(),
                default_policy: LinkPolicy::PERFECT,
                scripted_delays: VecDeque::new(),
                duplicate_budget: 0,
                corrupt_budget: 0,
                queue: Vec::new(),
                next_frame_id: 0,
                relaying_enabled: true,
                direct_requires_session: true,
            }),
        }
    }

    // ---------------------------------------------------------------- peers

    /// Enters `peer` into the simulation, online, on LAN segment 0, in
    /// partition group 0, reachable at `sim://<label>`.
    pub fn register(&self, peer: PeerId, label: &str) {
        let mut state = guard(&self.state);
        let address = direct_address(label);

        state
            .addresses
            .insert(address.clone(), EndpointRoute::Direct(peer));
        state.peers.insert(
            peer,
            PeerSlot {
                label: label.to_owned(),
                online: true,
                can_listen: true,
                announce_refused: false,
                relay_capable: true,
                lan_segment: 0,
                partition_group: 0,
                announce_generation: 0,
                announced_endpoints: vec![endpoint(&address, Reachability::Direct)],
                observed: BTreeMap::new(),
                sessions: BTreeSet::new(),
                relays: BTreeSet::new(),
            },
        );
    }

    /// Every peer in the simulation, in `PeerId` order.
    pub fn peers(&self) -> Vec<PeerId> {
        guard(&self.state).peers.keys().copied().collect()
    }

    /// Whether `peer` is in the simulation at all.
    pub fn is_registered(&self, peer: PeerId) -> bool {
        guard(&self.state).peers.contains_key(&peer)
    }

    /// Whether `peer`'s process is running.
    pub fn is_online(&self, peer: PeerId) -> bool {
        guard(&self.state)
            .peers
            .get(&peer)
            .is_some_and(|slot| slot.online)
    }

    /// The name `peer` was registered under.
    pub fn label_of(&self, peer: PeerId) -> Option<String> {
        guard(&self.state)
            .peers
            .get(&peer)
            .map(|slot| slot.label.clone())
    }

    /// Starts or stops `peer`'s process.
    ///
    /// Stopping is abrupt: nothing is announced and every link it held is
    /// dropped from both sides, so its neighbours learn of the departure by
    /// presence expiry (AC5) rather than by a courtesy frame. A graceful
    /// departure is `leave_network()` through the real port, which closes
    /// sessions properly.
    pub fn set_online(&self, peer: PeerId, online: bool) {
        let mut state = guard(&self.state);

        if let Some(slot) = state.peers.get_mut(&peer) {
            slot.online = online;
        }

        if !online {
            state.tear_down_links(peer);
        }
    }

    /// Discards the process-local network state of `peer`: its links and
    /// everything it had observed.
    ///
    /// The parts of a peer that outlive a process — its identity, peer cache,
    /// trust records, and sequence counter — are held by the harness and are
    /// untouched here (D12).
    pub fn reset_peer(&self, peer: PeerId) {
        let mut state = guard(&self.state);
        state.tear_down_links(peer);

        if let Some(slot) = state.peers.get_mut(&peer) {
            slot.observed.clear();
        }
    }

    /// Whether a transport link exists between the two peers.
    pub fn has_link(&self, from: PeerId, to: PeerId) -> bool {
        guard(&self.state)
            .peers
            .get(&from)
            .is_some_and(|slot| slot.sessions.contains(&to))
    }

    /// The peers `peer` holds a transport link with, in `PeerId` order.
    pub fn links_of(&self, peer: PeerId) -> Vec<PeerId> {
        guard(&self.state)
            .peers
            .get(&peer)
            .map(|slot| slot.sessions.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The addresses `peer` is reachable at: its direct address first, then one
    /// relayed address per advertised relay, in `PeerId` order.
    pub fn endpoints_of(&self, peer: PeerId) -> Vec<Endpoint> {
        guard(&self.state).endpoints_of(peer)
    }

    // -------------------------------------------------------------- shaping

    /// The policy every link without one of its own follows.
    pub fn set_default_policy(&self, policy: LinkPolicy) {
        guard(&self.state).default_policy = policy;
    }

    /// The latency every link without one of its own has.
    pub fn set_default_delay(&self, millis: u64) {
        let mut state = guard(&self.state);
        state.default_policy.delay = millis;
    }

    /// Replaces the policy of one directed link.
    pub fn set_link_policy(&self, from: PeerId, to: PeerId, policy: LinkPolicy) {
        guard(&self.state).links.insert((from, to), policy);
    }

    /// The policy of one directed link.
    pub fn link_policy(&self, from: PeerId, to: PeerId) -> LinkPolicy {
        guard(&self.state).policy(from, to)
    }

    /// Sets the latency of one directed link, leaving its other properties
    /// alone.
    pub fn set_link_delay(&self, from: PeerId, to: PeerId, millis: u64) {
        let mut state = guard(&self.state);
        let mut policy = state.policy(from, to);
        policy.delay = millis;
        state.links.insert((from, to), policy);
    }

    /// Queues per-message delays, consumed one per message frame in the order
    /// the frames are handed to the transport.
    ///
    /// This is how a scenario writes down a delivery order. Handing three
    /// messages to the transport against a script of `[30, 10, 20]` makes them
    /// arrive second, third, first — deterministically, with no clock and no
    /// chance involved. When the script runs out, links fall back to their own
    /// latency.
    ///
    /// Session, acknowledgement, and heartbeat frames deliberately do not
    /// consume the script: a scenario describing three messages should not have
    /// its script eaten by handshake traffic it never wrote down.
    pub fn script_delays(&self, delays: impl IntoIterator<Item = u64>) {
        guard(&self.state).scripted_delays.extend(delays);
    }

    /// How much of the delay script is still unconsumed.
    pub fn scripted_delays_remaining(&self) -> usize {
        guard(&self.state).scripted_delays.len()
    }

    /// Makes the next `count` message frames arrive twice (AC7).
    ///
    /// A budget rather than a link property, so a scenario can duplicate one
    /// specific message without every later one arriving twice.
    pub fn duplicate_next(&self, count: usize) {
        guard(&self.state).duplicate_budget = count;
    }

    /// Makes every message frame on one directed link arrive `copies` extra
    /// times.
    pub fn set_duplicates(&self, from: PeerId, to: PeerId, copies: u8) {
        let mut state = guard(&self.state);
        let mut policy = state.policy(from, to);
        policy.duplicates = copies;
        state.links.insert((from, to), policy);
    }

    /// Flips a signature bit on the next `count` message frames (AC6).
    ///
    /// The in-band way to stage a forgery: the envelope is genuine until it
    /// reaches the wire and corrupt when it arrives, so the recipient refuses
    /// it at exactly the boundary invariant 10 names.
    pub fn corrupt_next_signatures(&self, count: usize) {
        guard(&self.state).corrupt_budget = count;
    }

    /// Cuts one directed link. The rest of the network is untouched, so a
    /// third peer may still relay around it (AC12).
    pub fn sever_link_one_way(&self, from: PeerId, to: PeerId) {
        let mut state = guard(&self.state);
        let mut policy = state.policy(from, to);
        policy.severed = true;
        state.links.insert((from, to), policy);
    }

    /// Cuts the link in both directions.
    pub fn sever_link(&self, a: PeerId, b: PeerId) {
        self.sever_link_one_way(a, b);
        self.sever_link_one_way(b, a);
    }

    /// Restores a severed link in both directions.
    pub fn restore_link(&self, a: PeerId, b: PeerId) {
        let mut state = guard(&self.state);

        for pair in [(a, b), (b, a)] {
            let mut policy = state.policy(pair.0, pair.1);
            policy.severed = false;
            state.links.insert(pair, policy);
        }
    }

    /// Makes dials along one directed link fail in the stated way.
    pub fn set_dial_fault(&self, from: PeerId, to: PeerId, fault: DialFault) {
        let mut state = guard(&self.state);
        let mut policy = state.policy(from, to);
        policy.dial_fault = fault;
        state.links.insert((from, to), policy);
    }

    /// Moves `peer` to one side of a network split.
    ///
    /// Peers in different groups exchange nothing — no frames, no discovery, no
    /// relay path. Everything starts in group 0.
    pub fn set_partition_group(&self, peer: PeerId, group: u32) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&peer) {
            slot.partition_group = group;
        }
    }

    /// Which side of a split `peer` is on.
    pub fn partition_group(&self, peer: PeerId) -> u32 {
        guard(&self.state)
            .peers
            .get(&peer)
            .map_or(0, |slot| slot.partition_group)
    }

    /// Puts every peer back in one group.
    pub fn heal_partitions(&self) {
        for slot in guard(&self.state).peers.values_mut() {
            slot.partition_group = 0;
        }
    }

    /// Moves `peer` onto a broadcast domain.
    ///
    /// Discovery reaches only peers on the same segment, which is what makes
    /// AC2 ("two instances on the same LAN discover each other, unconfigured")
    /// and AC3 (an instance with no LAN neighbour, needing a ticket) two
    /// different scenarios rather than two different mocks.
    pub fn set_lan_segment(&self, peer: PeerId, segment: u32) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&peer) {
            slot.lan_segment = segment;
        }
    }

    /// Which broadcast domain `peer` is on.
    pub fn lan_segment(&self, peer: PeerId) -> u32 {
        guard(&self.state)
            .peers
            .get(&peer)
            .map_or(0, |slot| slot.lan_segment)
    }

    /// Whether `peer` offers circuit-relay service to others (AC4 says every
    /// instance does; this is how a scenario takes it away).
    pub fn set_relay_capable(&self, peer: PeerId, capable: bool) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&peer) {
            slot.relay_capable = capable;
        }
    }

    /// Whether the fabric will route a direct message around a severed link at
    /// all (S7: with no relay, two unreachable peers simply cannot connect).
    pub fn set_relaying_enabled(&self, enabled: bool) {
        guard(&self.state).relaying_enabled = enabled;
    }

    /// Whether a 1:1 message requires a transport link to the recipient.
    ///
    /// True by default, matching D4: a direct message travels over the
    /// authenticated session, and a send with no session is `SessionClosed`
    /// rather than a message that silently vanishes.
    pub fn set_direct_requires_session(&self, required: bool) {
        guard(&self.state).direct_requires_session = required;
    }

    /// Publishes a relayed address for `target` through `via` (AC12).
    ///
    /// The address appears in `target`'s endpoint list, so discovery hands it
    /// out and a dialer that cannot reach `target` directly can still reach it
    /// through `via`. The relay carries the frame unread — this fabric never
    /// looks inside an envelope it routes.
    pub fn advertise_relay(&self, target: PeerId, via: PeerId) {
        let mut state = guard(&self.state);

        let (Some(target_label), Some(via_label)) = (
            state.peers.get(&target).map(|slot| slot.label.clone()),
            state.peers.get(&via).map(|slot| slot.label.clone()),
        ) else {
            return;
        };

        state.addresses.insert(
            relayed_address(&via_label, &target_label),
            EndpointRoute::Relayed { target, via },
        );

        if let Some(slot) = state.peers.get_mut(&target) {
            slot.relays.insert(via);
        }
    }

    /// Withdraws a relayed address published by
    /// [`advertise_relay`](Self::advertise_relay).
    pub fn withdraw_relay(&self, target: PeerId, via: PeerId) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&target) {
            slot.relays.remove(&via);
        }
    }

    /// Whether `peer`'s transport can accept inbound links.
    ///
    /// A peer that cannot listen still dials out — it just is never dialled
    /// back, which the join diagnostic states rather than leaving a user to
    /// infer from a peer count that never grows (AC3).
    pub fn set_can_listen(&self, peer: PeerId, can_listen: bool) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&peer) {
            slot.can_listen = can_listen;
        }
    }

    /// Whether `peer`'s announcements are refused by the discovery mechanism.
    pub fn set_announce_refused(&self, peer: PeerId, refused: bool) {
        if let Some(slot) = guard(&self.state).peers.get_mut(&peer) {
            slot.announce_refused = refused;
        }
    }

    /// Re-announces every online peer that has announced at least once.
    ///
    /// The simulated mDNS tick. `observe_peers` reports a peer once per
    /// announcement, so without this a peer already seen stays unseen — which
    /// is faithful to the port's "since the last call" contract and would
    /// otherwise make a re-join look like an empty LAN.
    pub fn mdns_tick(&self) {
        for slot in guard(&self.state).peers.values_mut() {
            if slot.online && slot.announce_generation > 0 {
                slot.announce_generation += 1;
            }
        }
    }

    // ------------------------------------------------------- transport port

    /// Backs `PeerTransportPort::listen`.
    pub fn listen(&self, peer: PeerId) -> Result<Vec<Endpoint>, PeerTransportError> {
        let state = guard(&self.state);
        let slot = state
            .peers
            .get(&peer)
            .ok_or(PeerTransportError::Unavailable)?;

        if !slot.online {
            return Err(PeerTransportError::Unavailable);
        }
        if !slot.can_listen {
            return Err(PeerTransportError::ListenFailed);
        }

        Ok(state.endpoints_of(peer))
    }

    /// Backs `PeerTransportPort::dial`.
    ///
    /// Tries the endpoints in order and stops at the first that answers,
    /// reporting *which* one did — a relayed endpoint means a third peer is
    /// carrying the traffic, which the UI must be able to show (AC12).
    ///
    /// A successful dial is a completed handshake, so it puts both
    /// `SessionOpened` and `SessionEstablished` in flight to the far side.
    pub fn dial(
        &self,
        from: PeerId,
        to: PeerId,
        endpoints: &[Endpoint],
    ) -> Result<Endpoint, PeerTransportError> {
        let mut state = guard(&self.state);
        let now = self.clock.now_millis();

        if !state.peers.get(&from).is_some_and(|slot| slot.online) {
            return Err(PeerTransportError::Unavailable);
        }
        if !state.peers.contains_key(&to) {
            return Err(PeerTransportError::NoReachableEndpoint);
        }

        let mut answered = None;
        let mut fault = None;

        for candidate in endpoints {
            match state.endpoint_verdict(from, to, candidate) {
                Some(DialVerdict::Answered) => {
                    answered = Some(candidate.clone());
                    break;
                }
                Some(DialVerdict::HandshakeFailed) => {
                    fault = Some(PeerTransportError::HandshakeFailed);
                }
                Some(DialVerdict::Silent) | None => {}
            }
        }

        let Some(endpoint) = answered else {
            return Err(fault.unwrap_or(PeerTransportError::NoReachableEndpoint));
        };

        state.open_link(from, to);

        let dialer_endpoints = state.endpoints_of(from);
        state.push(
            now,
            from,
            to,
            SimFrame::SessionOpened {
                endpoints: dialer_endpoints,
            },
        );
        state.push(now, from, to, SimFrame::SessionEstablished);

        Ok(endpoint)
    }

    /// Backs `PeerTransportPort::close_session`.
    pub fn close(&self, from: PeerId, to: PeerId) -> Result<(), PeerTransportError> {
        let mut state = guard(&self.state);
        let now = self.clock.now_millis();

        if !state
            .peers
            .get(&from)
            .is_some_and(|slot| slot.sessions.contains(&to))
        {
            return Err(PeerTransportError::NoSuchSession);
        }

        state.close_link(from, to);
        state.push(now, from, to, SimFrame::SessionClosed);
        Ok(())
    }

    // ------------------------------------------------------- discovery port

    /// Backs `PeerDiscoveryPort::announce`.
    pub fn announce(&self, peer: PeerId, endpoints: &[Endpoint]) -> Result<(), PeerDiscoveryError> {
        let mut state = guard(&self.state);
        let slot = state
            .peers
            .get_mut(&peer)
            .ok_or(PeerDiscoveryError::Unavailable)?;

        if !slot.online {
            return Err(PeerDiscoveryError::Unavailable);
        }
        if slot.announce_refused {
            return Err(PeerDiscoveryError::AnnouncementRejected);
        }

        if !endpoints.is_empty() {
            slot.announced_endpoints = endpoints.to_vec();
        }
        slot.announce_generation += 1;
        Ok(())
    }

    /// Backs `PeerDiscoveryPort::observe_peers`.
    ///
    /// Reports every peer that announced on the same LAN segment since this
    /// observer last looked. An empty result is success: a LAN with no
    /// neighbour is the ordinary state of a first launch.
    pub fn observe(&self, peer: PeerId) -> Result<Vec<DiscoveredPeer>, PeerDiscoveryError> {
        let mut state = guard(&self.state);
        let observer = state
            .peers
            .get(&peer)
            .ok_or(PeerDiscoveryError::Unavailable)?;

        if !observer.online {
            return Err(PeerDiscoveryError::Unavailable);
        }

        let segment = observer.lan_segment;
        let group = observer.partition_group;
        let already_seen = observer.observed.clone();

        let mut discovered = Vec::new();
        let mut updates = Vec::new();

        for (other, slot) in &state.peers {
            let unseen_generation = slot.announce_generation;

            if *other == peer
                || !slot.online
                || unseen_generation == 0
                || slot.lan_segment != segment
                || slot.partition_group != group
                || already_seen.get(other).copied().unwrap_or(0) >= unseen_generation
            {
                continue;
            }

            updates.push((*other, unseen_generation));
            discovered.push(DiscoveredPeer {
                peer: *other,
                endpoints: state.endpoints_of(*other),
            });
        }

        if let Some(observer) = state.peers.get_mut(&peer) {
            for (other, generation) in updates {
                observer.observed.insert(other, generation);
            }
        }

        Ok(discovered)
    }

    /// Backs `PeerDiscoveryPort::redeem_join_ticket`.
    ///
    /// Crosses LAN segments — that is the whole point of D1's third rung — but
    /// respects partitions and severed links: a ticket is a bootstrap hint, not
    /// a tunnel. Validity is *not* re-checked here; that is `JoinTicket::validate`,
    /// a pure domain rule the application applies first.
    pub fn redeem(
        &self,
        observer: PeerId,
        ticket: &JoinTicket,
    ) -> Result<DiscoveredPeer, PeerDiscoveryError> {
        let state = guard(&self.state);

        if !state.peers.get(&observer).is_some_and(|slot| slot.online) {
            return Err(PeerDiscoveryError::Unavailable);
        }

        let issuer = ticket.issuer();
        if !state.peers.get(&issuer).is_some_and(|slot| slot.online) {
            return Err(PeerDiscoveryError::TicketUnreachable);
        }

        let answers = ticket.endpoints().iter().any(|candidate| {
            state.endpoint_verdict(observer, issuer, candidate) == Some(DialVerdict::Answered)
        });

        if !answers {
            return Err(PeerDiscoveryError::TicketUnreachable);
        }

        Ok(DiscoveredPeer {
            peer: issuer,
            endpoints: ticket.endpoints().to_vec(),
        })
    }

    // ------------------------------------------------- message transport port

    /// Backs `MessageTransportPort::send_direct`.
    ///
    /// Every refusal maps onto a `DeliveryFailure` the user can read (AC11),
    /// and the mapping is the honest one: an unreachable peer is not a missing
    /// relay, and a missing relay is not a closed session.
    pub fn send_direct(
        &self,
        from: PeerId,
        to: PeerId,
        envelope: &Envelope,
    ) -> Result<(), MessageTransportError> {
        let mut state = guard(&self.state);
        let now = self.clock.now_millis();

        let Some(source) = state.peers.get(&from) else {
            return Err(MessageTransportError::Unavailable);
        };
        if !source.online {
            return Err(MessageTransportError::Unavailable);
        }

        let holds_session = source.sessions.contains(&to);

        let Some(target) = state.peers.get(&to) else {
            return Err(MessageTransportError::PeerUnreachable);
        };
        if !target.online || target.partition_group != source.partition_group {
            return Err(MessageTransportError::PeerUnreachable);
        }

        if state.direct_requires_session && !holds_session {
            return Err(MessageTransportError::SessionClosed);
        }

        let routable = state.deliverable(from, to)
            || (state.relaying_enabled && state.find_relay(from, to).is_some());
        if !routable {
            return Err(MessageTransportError::NoRelayAvailable);
        }

        state.enqueue_message(now, from, to, envelope);
        Ok(())
    }

    /// Backs `MessageTransportPort::publish_broadcast`.
    ///
    /// Gossip, modelled as reachability: the message goes to every online peer
    /// the sender can reach through unsevered links inside its own partition,
    /// directly or through other peers (D3, AC10). Reaching nobody is success —
    /// a topic with no subscribers is not a failure, and a peer alone on the
    /// network is `Isolated` rather than broken.
    pub fn publish_broadcast(
        &self,
        from: PeerId,
        envelope: &Envelope,
    ) -> Result<(), MessageTransportError> {
        let mut state = guard(&self.state);
        let now = self.clock.now_millis();

        if !state.peers.get(&from).is_some_and(|slot| slot.online) {
            return Err(MessageTransportError::Unavailable);
        }

        for recipient in state.gossip_reach(from) {
            state.enqueue_message(now, from, recipient, envelope);
        }

        Ok(())
    }

    // ----------------------------------------------------------------- pump

    /// Puts one frame in flight from outside the ports.
    ///
    /// The harness uses this for the two frames no port produces: the
    /// acknowledgement that turns a `Pending` direct message into `Delivered`
    /// (AC11), and the heartbeat that keeps presence fresh (AC5).
    pub fn enqueue(&self, from: PeerId, to: PeerId, frame: SimFrame) {
        let mut state = guard(&self.state);
        let now = self.clock.now_millis();
        state.push(now, from, to, frame);
    }

    /// Removes and returns the next frame due at `now`, or `None` when nothing
    /// is due.
    ///
    /// "Due" means its delay has elapsed on the virtual clock. A frame
    /// scheduled into the future stays queued until the clock reaches it, which
    /// is why advancing time and pumping are separate steps.
    pub fn take_due(&self, now: u64) -> Option<QueuedFrame> {
        let mut state = guard(&self.state);

        let index = state
            .queue
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.due_at <= now)
            .min_by_key(|(_, frame)| frame.key())
            .map(|(index, _)| index)?;

        Some(state.queue.swap_remove(index))
    }

    /// How many frames are in flight, due or not.
    pub fn pending_frames(&self) -> usize {
        guard(&self.state).queue.len()
    }

    /// The earliest instant at which any queued frame becomes deliverable.
    pub fn next_due_at(&self) -> Option<u64> {
        guard(&self.state)
            .queue
            .iter()
            .map(|frame| frame.due_at)
            .min()
    }

    /// Why a frame from `from` cannot be handed to `to` right now, or `None`
    /// when it can.
    ///
    /// Evaluated at delivery rather than at enqueue, so a partition that comes
    /// down while a frame is in flight drops it — which is what a partition
    /// does to traffic already on the wire.
    pub fn delivery_block(&self, from: PeerId, to: PeerId) -> Option<DropCause> {
        let state = guard(&self.state);

        let (Some(source), Some(target)) = (state.peers.get(&from), state.peers.get(&to)) else {
            return Some(DropCause::DestinationUnknown);
        };

        if !target.online {
            return Some(DropCause::DestinationOffline);
        }
        if source.partition_group != target.partition_group {
            return Some(DropCause::Partitioned);
        }
        if state.policy(from, to).severed && state.find_relay(from, to).is_none() {
            return Some(DropCause::LinkSevered);
        }

        None
    }

    // ------------------------------------------------------------ randomness

    /// A seeded draw below `bound` — the only chance in the simulation.
    pub fn random_below(&self, bound: u64) -> u64 {
        guard(&self.state).rng.below(bound)
    }

    /// Shuffles `items` from the seeded stream.
    pub fn shuffle<T>(&self, items: &mut [T]) {
        guard(&self.state).rng.shuffle(items);
    }
}

impl FabricState {
    fn policy(&self, from: PeerId, to: PeerId) -> LinkPolicy {
        self.links
            .get(&(from, to))
            .copied()
            .unwrap_or(self.default_policy)
    }

    /// Whether a frame handed over now would travel `from` → `to` directly.
    fn deliverable(&self, from: PeerId, to: PeerId) -> bool {
        let (Some(source), Some(target)) = (self.peers.get(&from), self.peers.get(&to)) else {
            return false;
        };

        source.online
            && target.online
            && source.partition_group == target.partition_group
            && !self.policy(from, to).severed
    }

    /// The lowest-`PeerId` peer that could carry traffic between two peers a
    /// severed link separates (AC12).
    ///
    /// Lowest rather than random on purpose: which peer relays is not the thing
    /// a scenario is testing, and a stable choice keeps the trace readable.
    fn find_relay(&self, from: PeerId, to: PeerId) -> Option<PeerId> {
        self.peers
            .iter()
            .find(|(candidate, slot)| {
                **candidate != from
                    && **candidate != to
                    && slot.online
                    && slot.relay_capable
                    && self.deliverable(from, **candidate)
                    && self.deliverable(**candidate, to)
            })
            .map(|(candidate, _)| *candidate)
    }

    /// Every peer a broadcast from `origin` reaches, in `PeerId` order.
    fn gossip_reach(&self, origin: PeerId) -> Vec<PeerId> {
        let mut reached = BTreeSet::new();
        let mut frontier = vec![origin];

        while let Some(node) = frontier.pop() {
            for candidate in self.peers.keys() {
                if *candidate == origin
                    || reached.contains(candidate)
                    || !self.deliverable(node, *candidate)
                {
                    continue;
                }

                reached.insert(*candidate);
                frontier.push(*candidate);
            }
        }

        reached.into_iter().collect()
    }

    fn endpoints_of(&self, peer: PeerId) -> Vec<Endpoint> {
        let Some(slot) = self.peers.get(&peer) else {
            return Vec::new();
        };

        let mut endpoints = slot.announced_endpoints.clone();

        for relay in &slot.relays {
            if let Some(via) = self.peers.get(relay) {
                endpoints.push(endpoint(
                    &relayed_address(&via.label, &slot.label),
                    Reachability::Relayed,
                ));
            }
        }

        endpoints
    }

    /// What dialling `to` at `candidate` would produce, or `None` when the
    /// address does not belong to `to` at all.
    fn endpoint_verdict(
        &self,
        from: PeerId,
        to: PeerId,
        candidate: &Endpoint,
    ) -> Option<DialVerdict> {
        match self.addresses.get(candidate.address()).copied()? {
            EndpointRoute::Direct(target) if target == to => Some(self.dial_verdict(from, to)),
            EndpointRoute::Relayed { target, via } if target == to => {
                let usable = via != from
                    && via != to
                    && self.peers.get(&via).is_some_and(|slot| slot.relay_capable)
                    && self.dial_verdict(from, via) == DialVerdict::Answered
                    && self.dial_verdict(via, to) == DialVerdict::Answered;

                Some(if usable {
                    DialVerdict::Answered
                } else {
                    DialVerdict::Silent
                })
            }
            _ => None,
        }
    }

    /// What a direct dial between two peers would produce.
    ///
    /// A handshake needs both directions, so a link severed either way is
    /// silent.
    fn dial_verdict(&self, from: PeerId, to: PeerId) -> DialVerdict {
        if !self.deliverable(from, to) || !self.deliverable(to, from) {
            return DialVerdict::Silent;
        }

        let faults = [
            self.policy(from, to).dial_fault,
            self.policy(to, from).dial_fault,
        ];

        if faults.contains(&DialFault::HandshakeFailure) {
            DialVerdict::HandshakeFailed
        } else if faults.contains(&DialFault::Unreachable) {
            DialVerdict::Silent
        } else {
            DialVerdict::Answered
        }
    }

    fn open_link(&mut self, from: PeerId, to: PeerId) {
        if let Some(slot) = self.peers.get_mut(&from) {
            slot.sessions.insert(to);
        }
        if let Some(slot) = self.peers.get_mut(&to) {
            slot.sessions.insert(from);
        }
    }

    fn close_link(&mut self, from: PeerId, to: PeerId) {
        if let Some(slot) = self.peers.get_mut(&from) {
            slot.sessions.remove(&to);
        }
        if let Some(slot) = self.peers.get_mut(&to) {
            slot.sessions.remove(&from);
        }
    }

    fn tear_down_links(&mut self, peer: PeerId) {
        let held: Vec<PeerId> = self
            .peers
            .get(&peer)
            .map(|slot| slot.sessions.iter().copied().collect())
            .unwrap_or_default();

        for other in held {
            self.close_link(peer, other);
        }
    }

    /// Puts a message frame in flight, applying corruption and duplication.
    fn enqueue_message(&mut self, now: u64, from: PeerId, to: PeerId, envelope: &Envelope) {
        let mut envelope = envelope.clone();

        if self.corrupt_budget > 0 {
            self.corrupt_budget -= 1;
            corrupt_signature(&mut envelope);
        }

        let mut copies = 1 + usize::from(self.policy(from, to).duplicates);
        if self.duplicate_budget > 0 {
            self.duplicate_budget -= 1;
            copies += 1;
        }

        for _ in 0..copies {
            self.push(now, from, to, SimFrame::Message(envelope.clone()));
        }
    }

    fn push(&mut self, now: u64, from: PeerId, to: PeerId, frame: SimFrame) {
        let delay = if frame.is_message() {
            self.scripted_delays
                .pop_front()
                .unwrap_or_else(|| self.policy(from, to).delay)
        } else {
            self.policy(from, to).delay
        };

        let id = self.next_frame_id;
        self.next_frame_id += 1;

        self.queue.push(QueuedFrame {
            id,
            due_at: now.saturating_add(delay),
            from,
            to,
            frame,
        });
    }
}

/// The address a peer is directly reachable at.
fn direct_address(label: &str) -> String {
    format!("sim://{label}")
}

/// The address a peer is reachable at through a relaying peer.
///
/// Shaped after a libp2p circuit multiaddress so a scenario's trace reads like
/// the real thing, but parsed by nothing: this fabric resolves addresses by
/// exact match against its own directory.
fn relayed_address(via: &str, target: &str) -> String {
    format!("sim://{via}/p2p-circuit/{target}")
}

fn endpoint(address: &str, reachability: Reachability) -> Endpoint {
    Endpoint::new(address, reachability).expect("a simulated address is always admissible")
}

/// Flips one signature bit, turning a genuine envelope into a forged one.
fn corrupt_signature(envelope: &mut Envelope) {
    let mut bytes = *envelope.signature.as_bytes();
    bytes[0] ^= 0x01;
    envelope.signature = EnvelopeSignature::new(bytes);
}
