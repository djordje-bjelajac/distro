use std::collections::{HashMap, HashSet};
use std::sync::mpsc::SyncSender;
use std::time::Instant;

use futures::StreamExt;
use libp2p::core::ConnectedPoint;
use libp2p::core::transport::ListenerId;
use libp2p::gossipsub::{IdentTopic, PublishError};
use libp2p::request_response::{Message as RequestResponseMessage, OutboundRequestId};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{ConnectionId, DialError, Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId as Libp2pPeerId, identify, kad, mdns, request_response};
use membership::domain::{Endpoint, SessionDirection};
use membership::ports::{DiscoveredPeer, PeerDiscoveryError, PeerTransportError};
use messaging::ports::MessageTransportError;
use shared_types::{EnvelopeSignature, PeerId};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::limits::{InboundRateLimiter, ResourceLimits};
use crate::mapping::{EndpointMapping, PeerIdMapping};
use crate::swarm::direct_message_codec::DirectMessageAck;
use crate::swarm::distro_behaviour::{DistroBehaviour, DistroBehaviourEvent};
use crate::swarm::link_registry::LinkRegistry;
use crate::swarm::network_command::{NetworkCommand, Reply};
use crate::swarm::network_event::{DirectMessageFailure, NetworkEvent};

/// The one task that owns the swarm.
///
/// # The threading contract, in one paragraph
///
/// A `Swarm` is `!Sync` in practice — it is a state machine that must be polled
/// from exactly one place — while the ports it serves are synchronous, take
/// `&self`, and are called from whatever thread the composition root is running
/// on. This type is the seam. It owns the swarm outright, runs as a single
/// `tokio` task, and is reachable only through two channels: commands in
/// (`tokio::sync::mpsc`, unbounded, so a port call never blocks on the send)
/// and events out (`std::sync::mpsc`, bounded, so a root that stops draining
/// cannot grow this process without limit). Nothing else touches the swarm, and
/// no `libp2p` type appears on either channel.
///
/// # Why the replies are `std` channels
///
/// A port method blocks on its reply. `tokio::sync::oneshot::blocking_recv`
/// panics if it is called from inside a runtime; `std::sync::mpsc::recv_timeout`
/// blocks anywhere and, more importantly, *gives up* — which is what turns a
/// dead driver into a typed error rather than a hung application (AC3).
///
/// # Why the collapse rule is applied here
///
/// [`LinkRegistry`] resolves a simultaneous connect the instant the second
/// connection appears, using the domain's own rule, and closes the superseded
/// link itself. Everything above this line therefore sees at most one session
/// per peer — which is exactly what `PeerTransportPort::close_session`, closing
/// *by peer*, is able to express.
pub(crate) struct NetworkDriver {
    swarm: Swarm<DistroBehaviour>,
    topic: IdentTopic,
    listen_addresses: Vec<Multiaddr>,
    limits: ResourceLimits,
    codec: EnvelopeCodec,
    diagnostics: CodecDiagnostics,
    rate_limiter: InboundRateLimiter,
    links: LinkRegistry,
    started_at: Instant,

    commands: UnboundedReceiver<NetworkCommand>,
    events: SyncSender<NetworkEvent>,

    listening: Vec<Multiaddr>,
    pending_listen: Option<PendingListen>,
    pending_dials: HashMap<Libp2pPeerId, Vec<PendingDial>>,
    known_addresses: HashMap<Libp2pPeerId, Vec<Multiaddr>>,
    observed: Vec<DiscoveredPeer>,
    announced: Vec<Multiaddr>,
    outbound_direct: HashMap<OutboundRequestId, (PeerId, EnvelopeSignature)>,
}

struct PendingListen {
    awaiting: HashSet<ListenerId>,
    reply: Reply<Vec<Endpoint>, PeerTransportError>,
}

/// A caller blocked on a dial. The two shapes exist because the same dial
/// answers two different ports, each with its own error vocabulary.
enum PendingDial {
    /// `PeerTransportPort::dial` — wants to know *which* endpoint answered.
    Transport(Reply<Endpoint, PeerTransportError>),
    /// `PeerDiscoveryPort::redeem_join_ticket` — wants the peer it found.
    Ticket {
        peer: PeerId,
        endpoints: Vec<Endpoint>,
        reply: Reply<DiscoveredPeer, PeerDiscoveryError>,
    },
}

impl NetworkDriver {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        swarm: Swarm<DistroBehaviour>,
        local: PeerId,
        topic: IdentTopic,
        listen_addresses: Vec<Multiaddr>,
        limits: ResourceLimits,
        codec: EnvelopeCodec,
        diagnostics: CodecDiagnostics,
        commands: UnboundedReceiver<NetworkCommand>,
        events: SyncSender<NetworkEvent>,
    ) -> Self {
        Self {
            swarm,
            topic,
            listen_addresses,
            limits,
            codec,
            diagnostics,
            rate_limiter: InboundRateLimiter::new(
                limits.inbound_envelopes_per_second,
                limits.inbound_envelope_burst,
            ),
            links: LinkRegistry::new(local),
            started_at: Instant::now(),
            commands,
            events,
            listening: Vec::new(),
            pending_listen: None,
            pending_dials: HashMap::new(),
            known_addresses: HashMap::new(),
            observed: Vec::new(),
            announced: Vec::new(),
            outbound_direct: HashMap::new(),
        }
    }

    /// Runs until the runtime asks it to stop or the command channel closes.
    pub(crate) async fn run(mut self) {
        loop {
            tokio::select! {
                command = self.commands.recv() => match command {
                    Some(NetworkCommand::Shutdown) | None => break,
                    Some(command) => self.handle_command(command),
                },
                event = self.swarm.select_next_some() => self.handle_swarm_event(event),
            }
        }
    }

    // ------------------------------------------------------------- commands

    fn handle_command(&mut self, command: NetworkCommand) {
        match command {
            NetworkCommand::Listen { reply } => self.start_listening(reply),
            NetworkCommand::Dial {
                peer,
                endpoints,
                reply,
            } => self.dial(peer, endpoints, reply),
            NetworkCommand::CloseSession { peer, reply } => self.close_session(peer, &reply),
            NetworkCommand::Announce { endpoints, reply } => self.announce(&endpoints, &reply),
            NetworkCommand::ObservePeers { reply } => {
                answer(&reply, Ok(std::mem::take(&mut self.observed)));
            }
            NetworkCommand::RedeemTicket { ticket, reply } => self.redeem(*ticket, reply),
            NetworkCommand::SendDirect {
                to,
                signature,
                frame,
                reply,
            } => self.send_direct(to, signature, frame, &reply),
            NetworkCommand::PublishBroadcast { frame, reply } => {
                self.publish_broadcast(frame, &reply);
            }
            NetworkCommand::Shutdown => {}
        }
    }

    fn start_listening(&mut self, reply: Reply<Vec<Endpoint>, PeerTransportError>) {
        if !self.listening.is_empty() {
            answer(&reply, Ok(self.local_endpoints()));
            return;
        }

        let mut awaiting = HashSet::new();
        for address in self.listen_addresses.clone() {
            if let Ok(listener) = self.swarm.listen_on(address) {
                awaiting.insert(listener);
            }
        }

        if awaiting.is_empty() {
            answer(&reply, Err(PeerTransportError::ListenFailed));
            return;
        }

        self.pending_listen = Some(PendingListen { awaiting, reply });
    }

    fn dial(
        &mut self,
        peer: PeerId,
        endpoints: Vec<Endpoint>,
        reply: Reply<Endpoint, PeerTransportError>,
    ) {
        let Ok(remote) = PeerIdMapping::to_libp2p(peer) else {
            answer(&reply, Err(PeerTransportError::Unavailable));
            return;
        };

        // Dialling a peer we already hold a session with is idempotent: the
        // caller asked for a link and there is one. Reporting the surviving
        // link's address rather than re-dialling is what keeps a roster refresh
        // from opening a second connection every time.
        if let Some(address) = self.links.primary_address(&remote) {
            answer(
                &reply,
                endpoint_of(&address).ok_or(PeerTransportError::Unavailable),
            );
            return;
        }

        let addresses = self.dialable(&endpoints);
        if addresses.is_empty() {
            answer(&reply, Err(PeerTransportError::NoReachableEndpoint));
            return;
        }

        self.remember(remote, &addresses);
        match self.swarm.dial(
            DialOpts::peer_id(remote)
                .addresses(addresses)
                .condition(PeerCondition::Always)
                .build(),
        ) {
            Ok(()) => self
                .pending_dials
                .entry(remote)
                .or_default()
                .push(PendingDial::Transport(reply)),
            Err(error) => answer(&reply, Err(transport_dial_error(&error))),
        }
    }

    fn redeem(
        &mut self,
        ticket: membership::domain::JoinTicket,
        reply: Reply<DiscoveredPeer, PeerDiscoveryError>,
    ) {
        // Expiry and protocol compatibility are deliberately not re-checked
        // here: that is `JoinTicket::validate`, a pure domain rule the
        // application applies first. Checking it on both sides of the boundary
        // would put a clock on each and let the two disagree.
        let peer = ticket.issuer();
        let endpoints = ticket.endpoints().to_vec();

        let Ok(remote) = PeerIdMapping::to_libp2p(peer) else {
            answer(&reply, Err(PeerDiscoveryError::TicketUnreachable));
            return;
        };

        if self.links.holds_session(&remote) {
            answer(
                &reply,
                Ok(DiscoveredPeer {
                    peer,
                    endpoints: endpoints.clone(),
                }),
            );
            return;
        }

        let addresses = self.dialable(&endpoints);
        if addresses.is_empty() {
            answer(&reply, Err(PeerDiscoveryError::TicketUnreachable));
            return;
        }

        self.remember(remote, &addresses);
        match self.swarm.dial(
            DialOpts::peer_id(remote)
                .addresses(addresses)
                .condition(PeerCondition::Always)
                .build(),
        ) {
            Ok(()) => self
                .pending_dials
                .entry(remote)
                .or_default()
                .push(PendingDial::Ticket {
                    peer,
                    endpoints,
                    reply,
                }),
            Err(_) => answer(&reply, Err(PeerDiscoveryError::TicketUnreachable)),
        }
    }

    fn close_session(&mut self, peer: PeerId, reply: &Reply<(), PeerTransportError>) {
        let Ok(remote) = PeerIdMapping::to_libp2p(peer) else {
            answer(reply, Err(PeerTransportError::NoSuchSession));
            return;
        };

        // The registry, not the swarm, is what "holds a session" means here.
        // A connection the swarm is still winding down after an earlier close
        // is not a session anybody can use, and reporting `Ok` for it would
        // tell the roster it had just closed something that was already gone.
        if !self.links.holds_session(&remote) {
            answer(reply, Err(PeerTransportError::NoSuchSession));
            return;
        }

        let connections = self.links.connections_of(&remote);

        // Close by peer, which is what the port asks for and — because the
        // collapse was already resolved below this line — is now unambiguous.
        for connection in connections {
            self.swarm.close_connection(connection);
        }
        let _ = self.swarm.disconnect_peer_id(remote);

        self.links.forget(&remote);
        self.rate_limiter.forget(&remote);
        answer(reply, Ok(()));
    }

    fn announce(&mut self, endpoints: &[Endpoint], reply: &Reply<(), PeerDiscoveryError>) {
        let mut addresses = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints {
            match EndpointMapping::to_multiaddr(endpoint) {
                Ok(address) => addresses.push(address),
                Err(_) => {
                    answer(reply, Err(PeerDiscoveryError::AnnouncementRejected));
                    return;
                }
            }
        }

        for address in addresses {
            if !self.announced.contains(&address) {
                self.announced.push(address.clone());
            }
            self.swarm.add_external_address(address);
        }

        // Refresh the routing table so the peers we know learn where we are.
        // `NoKnownPeers` is the ordinary state of a first launch on an empty
        // LAN — `Isolated` is a normal status, not an error (AC3).
        let _ = self.swarm.behaviour_mut().kademlia.bootstrap();
        answer(reply, Ok(()));
    }

    fn send_direct(
        &mut self,
        to: PeerId,
        signature: EnvelopeSignature,
        frame: Vec<u8>,
        reply: &Reply<(), MessageTransportError>,
    ) {
        let Ok(remote) = PeerIdMapping::to_libp2p(to) else {
            answer(reply, Err(MessageTransportError::PeerUnreachable));
            return;
        };

        if frame.len() > self.limits.max_envelope_bytes {
            self.diagnostics.count_oversize_frame();
            answer(reply, Err(MessageTransportError::Unavailable));
            return;
        }

        // A peer with neither a live link nor a single known address has no
        // path at all, and saying so now is more honest than accepting the
        // message and failing it a timeout later (AC11).
        if !self.links.holds_session(&remote)
            && self
                .known_addresses
                .get(&remote)
                .is_none_or(|addresses| addresses.is_empty())
        {
            answer(reply, Err(MessageTransportError::PeerUnreachable));
            return;
        }

        let request = self
            .swarm
            .behaviour_mut()
            .direct
            .send_request(&remote, frame);
        self.outbound_direct.insert(request, (to, signature));
        answer(reply, Ok(()));
    }

    fn publish_broadcast(&mut self, frame: Vec<u8>, reply: &Reply<(), MessageTransportError>) {
        if frame.len() > self.limits.max_envelope_bytes {
            self.diagnostics.count_oversize_frame();
            answer(reply, Err(MessageTransportError::Unavailable));
            return;
        }

        let result = match self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), frame)
        {
            Ok(_) => Ok(()),
            // Reaching nobody is success: a topic with no subscribers is not a
            // failure, and a peer alone on the network is `Isolated` rather
            // than broken. The same rule the simulated fabric applies.
            Err(PublishError::NoPeersSubscribedToTopic | PublishError::Duplicate) => Ok(()),
            Err(_) => Err(MessageTransportError::Unavailable),
        };

        answer(reply, result);
    }

    // -------------------------------------------------------- swarm events

    fn handle_swarm_event(&mut self, event: SwarmEvent<DistroBehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => self.listener_ready(listener_id, address),
            SwarmEvent::ListenerError { listener_id, .. } => self.listener_done(listener_id),
            SwarmEvent::ListenerClosed { listener_id, .. } => self.listener_done(listener_id),
            SwarmEvent::ExternalAddrConfirmed { address } => {
                if !self.announced.contains(&address) {
                    self.announced.push(address.clone());
                }
                if let Some(endpoint) = endpoint_of(&address) {
                    self.emit(NetworkEvent::ExternalAddressConfirmed(endpoint));
                }
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => self.connection_established(peer_id, connection_id, &endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                connection_id,
                ..
            } => self.connection_closed(peer_id, connection_id),
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(peer),
                error,
                ..
            } => self.dial_failed(peer, &error),
            SwarmEvent::Behaviour(event) => self.handle_behaviour_event(event),
            _ => {}
        }
    }

    fn listener_ready(&mut self, listener: ListenerId, address: Multiaddr) {
        if !self.listening.contains(&address) {
            self.listening.push(address.clone());
        }
        if let Some(endpoint) = endpoint_of(&address) {
            self.emit(NetworkEvent::ListeningOn(endpoint));
        }
        self.listener_done(listener);
    }

    fn listener_done(&mut self, listener: ListenerId) {
        let Some(pending) = self.pending_listen.as_mut() else {
            return;
        };
        pending.awaiting.remove(&listener);
        if !pending.awaiting.is_empty() {
            return;
        }

        let pending = self.pending_listen.take().expect("checked just above");
        let endpoints = self.local_endpoints();
        answer(
            &pending.reply,
            if endpoints.is_empty() {
                Err(PeerTransportError::ListenFailed)
            } else {
                Ok(endpoints)
            },
        );
    }

    fn connection_established(
        &mut self,
        remote: Libp2pPeerId,
        connection: ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        let Ok(identity) = PeerIdMapping::from_libp2p(&remote) else {
            // A peer whose identity this build cannot express is one it cannot
            // attribute a message to (invariant 4), so the link is useless.
            self.swarm.close_connection(connection);
            return;
        };

        let (direction, address) = match endpoint {
            ConnectedPoint::Dialer { address, .. } => (SessionDirection::Outbound, address.clone()),
            ConnectedPoint::Listener { send_back_addr, .. } => {
                (SessionDirection::Inbound, send_back_addr.clone())
            }
        };

        self.remember(remote, std::slice::from_ref(&address));

        // Did *we* ask for this link? An outbound dial is a decision the
        // application already made and already knows the answer to —
        // `connect_to_peer` opens and establishes the session itself the
        // moment `dial` returns. Reporting it again as an inbound session
        // would have the roster see a second open for a peer it already holds
        // and collapse a link nobody asked it to.
        let we_dialled = self.pending_dials.contains_key(&remote);

        let Ok(outcome) =
            self.links
                .record_established(identity, remote, connection, direction, address.clone())
        else {
            // A connection claiming our own identity (invariant 2).
            self.swarm.close_connection(connection);
            return;
        };

        // The superseded link of a simultaneous connect, closed here and never
        // reported upward — the session lives on the survivor.
        for discarded in outcome.close {
            self.swarm.close_connection(discarded);
        }

        if let Some(replies) = self.pending_dials.remove(&remote) {
            for pending in replies {
                match pending {
                    PendingDial::Transport(reply) => answer(
                        &reply,
                        endpoint_of(&outcome.primary_address)
                            .ok_or(PeerTransportError::HandshakeFailed),
                    ),
                    PendingDial::Ticket {
                        peer,
                        endpoints,
                        reply,
                    } => answer(&reply, Ok(DiscoveredPeer { peer, endpoints })),
                }
            }
        }

        if outcome.newly_connected
            && !we_dialled
            && let Some(endpoint) = endpoint_of(&outcome.primary_address)
        {
            self.emit(NetworkEvent::SessionEstablished {
                peer: identity,
                endpoint,
            });
        }
    }

    fn connection_closed(&mut self, remote: Libp2pPeerId, connection: ConnectionId) {
        let Some(outcome) = self.links.record_closed(remote, connection) else {
            return;
        };
        if !outcome.peer_gone {
            return;
        }

        self.rate_limiter.forget(&remote);
        if let Ok(identity) = PeerIdMapping::from_libp2p(&remote) {
            self.emit(NetworkEvent::SessionClosed { peer: identity });
        }
    }

    fn dial_failed(&mut self, remote: Libp2pPeerId, error: &DialError) {
        let Some(replies) = self.pending_dials.remove(&remote) else {
            return;
        };

        for pending in replies {
            match pending {
                PendingDial::Transport(reply) => answer(&reply, Err(transport_dial_error(error))),
                PendingDial::Ticket { reply, .. } => {
                    answer(&reply, Err(PeerDiscoveryError::TicketUnreachable));
                }
            }
        }
    }

    // ---------------------------------------------------- behaviour events

    fn handle_behaviour_event(&mut self, event: DistroBehaviourEvent) {
        match event {
            DistroBehaviourEvent::Identify(identify::Event::Received { peer_id, info, .. }) => {
                self.record_discovery(peer_id, info.listen_addrs);
            }
            DistroBehaviourEvent::Kademlia(kad::Event::RoutingUpdated {
                peer, addresses, ..
            }) => {
                self.record_discovery(peer, addresses.into_vec());
            }
            DistroBehaviourEvent::Mdns(mdns::Event::Discovered(found)) => {
                let mut grouped: HashMap<Libp2pPeerId, Vec<Multiaddr>> = HashMap::new();
                for (peer, address) in found {
                    grouped.entry(peer).or_default().push(address);
                }
                for (peer, addresses) in grouped {
                    self.record_discovery(peer, addresses);
                }
            }
            DistroBehaviourEvent::Gossipsub(libp2p::gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            }) => {
                self.inbound_frame(propagation_source, &message.data);
            }
            DistroBehaviourEvent::Direct(request_response::Event::Message {
                peer,
                message,
                ..
            }) => self.direct_message(peer, message),
            DistroBehaviourEvent::Direct(request_response::Event::OutboundFailure {
                request_id,
                ..
            }) => self.direct_failed(request_id, DirectMessageFailure::NotAcknowledged),
            _ => {}
        }
    }

    fn direct_message(
        &mut self,
        remote: Libp2pPeerId,
        message: RequestResponseMessage<Vec<u8>, DirectMessageAck>,
    ) {
        match message {
            RequestResponseMessage::Request {
                request, channel, ..
            } => {
                let accepted = self.inbound_frame(remote, &request);
                let _ = self.swarm.behaviour_mut().direct.send_response(
                    channel,
                    if accepted {
                        DirectMessageAck::Accepted
                    } else {
                        DirectMessageAck::Refused
                    },
                );
            }
            RequestResponseMessage::Response {
                request_id,
                response,
            } => match response {
                DirectMessageAck::Accepted => {
                    if let Some((peer, signature)) = self.outbound_direct.remove(&request_id) {
                        self.emit(NetworkEvent::DirectMessageDelivered { peer, signature });
                    }
                }
                DirectMessageAck::Refused => {
                    self.direct_failed(request_id, DirectMessageFailure::Refused);
                }
            },
        }
    }

    fn direct_failed(&mut self, request: OutboundRequestId, reason: DirectMessageFailure) {
        if let Some((peer, signature)) = self.outbound_direct.remove(&request) {
            self.emit(NetworkEvent::DirectMessageFailed {
                peer,
                signature,
                reason,
            });
        }
    }

    /// Takes one frame off the wire: rate limit, then decode, then report.
    ///
    /// Returns whether the frame was taken in, which is what the direct-message
    /// acknowledgement carries back to the sender.
    fn inbound_frame(&mut self, remote: Libp2pPeerId, frame: &[u8]) -> bool {
        if !self.rate_limiter.admit(remote, self.elapsed_millis()) {
            self.diagnostics.count_rate_limited();
            return false;
        }

        let Ok(envelope) = self.codec.decode(frame) else {
            // Already counted by the codec, with the reason S2 requires.
            return false;
        };
        let Ok(from) = PeerIdMapping::from_libp2p(&remote) else {
            return false;
        };

        self.emit(NetworkEvent::EnvelopeReceived { from, envelope });
        true
    }

    // ------------------------------------------------------------- helpers

    fn record_discovery(&mut self, remote: Libp2pPeerId, addresses: Vec<Multiaddr>) {
        if addresses.is_empty() || remote == *self.swarm.local_peer_id() {
            return;
        }
        let Ok(peer) = PeerIdMapping::from_libp2p(&remote) else {
            return;
        };

        self.remember(remote, &addresses);
        for address in &addresses {
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&remote, address.clone());
        }

        let endpoints: Vec<Endpoint> = addresses.iter().filter_map(endpoint_of).collect();
        if endpoints.is_empty() {
            return;
        }

        let discovered = DiscoveredPeer {
            peer,
            endpoints: endpoints.clone(),
        };

        // The buffer `observe_peers` drains, and the pushed event, carry the
        // same sighting. A root uses one or the other, never both.
        match self
            .observed
            .iter_mut()
            .find(|existing| existing.peer == peer)
        {
            Some(existing) => {
                for endpoint in endpoints {
                    if !existing.endpoints.contains(&endpoint) {
                        existing.endpoints.push(endpoint);
                    }
                }
            }
            None => self.observed.push(discovered.clone()),
        }

        self.emit(NetworkEvent::PeerDiscovered(discovered));
    }

    fn remember(&mut self, remote: Libp2pPeerId, addresses: &[Multiaddr]) {
        let known = self.known_addresses.entry(remote).or_default();
        for address in addresses {
            if !known.contains(address) {
                known.push(address.clone());
            }
        }
    }

    /// The endpoints this peer is reachable at: what it is listening on, plus
    /// every address another peer has confirmed — which is where a relayed
    /// circuit address appears for a peer behind a NAT (AC12).
    fn local_endpoints(&self) -> Vec<Endpoint> {
        let mut endpoints: Vec<Endpoint> = Vec::new();
        for address in self.listening.iter().chain(self.announced.iter()) {
            if let Some(endpoint) = endpoint_of(address)
                && !endpoints.contains(&endpoint)
            {
                endpoints.push(endpoint);
            }
        }
        endpoints
    }

    fn dialable(&self, endpoints: &[Endpoint]) -> Vec<Multiaddr> {
        endpoints
            .iter()
            .filter_map(|endpoint| EndpointMapping::to_multiaddr(endpoint).ok())
            .collect()
    }

    /// Milliseconds since this driver started.
    ///
    /// A monotonic counter local to the adapter, used only for the rate
    /// limiter's bucket arithmetic. It is not a domain clock and no domain rule
    /// reads it — every time-dependent *rule* still goes through `ClockPort`
    /// (D11, S5).
    fn elapsed_millis(&self) -> u64 {
        u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn emit(&self, event: NetworkEvent) {
        if self.events.try_send(event).is_err() {
            // Counted, never silent: a root that stopped draining is a bug the
            // diagnostics pane can name (S6's bounded queue).
            self.diagnostics.count_dropped_event();
        }
    }
}

/// Hands a result back to a blocked caller, tolerating a caller that gave up.
///
/// A send failure means the port method's `recv_timeout` already elapsed and
/// returned its own refusal. Nothing to do, and nothing to log: the caller has
/// already been told.
fn answer<T, E>(reply: &Reply<T, E>, result: Result<T, E>) {
    let _ = reply.try_send(result);
}

fn endpoint_of(address: &Multiaddr) -> Option<Endpoint> {
    EndpointMapping::to_endpoint(address).ok()
}

/// Maps a libp2p dial failure onto the transport port's vocabulary.
///
/// The distinction that matters is "nothing answered" versus "something
/// answered and the handshake did not complete": the first is S7's known limit
/// with no relay available, the second is a peer that is there but wrong.
fn transport_dial_error(error: &DialError) -> PeerTransportError {
    match error {
        DialError::LocalPeerId { .. } => PeerTransportError::HandshakeFailed,
        DialError::WrongPeerId { .. } => PeerTransportError::HandshakeFailed,
        DialError::Denied { .. } => PeerTransportError::Unavailable,
        DialError::NoAddresses => PeerTransportError::NoReachableEndpoint,
        DialError::DialPeerConditionFalse(_) => PeerTransportError::NoReachableEndpoint,
        DialError::Aborted => PeerTransportError::NoReachableEndpoint,
        DialError::Transport(_) => PeerTransportError::NoReachableEndpoint,
    }
}
