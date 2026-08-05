use std::sync::Arc;

use infra_net_libp2p::swarm::NetworkEvent;
use membership::ports::{DiscoveredPeer, InboundSessionPort, PeerDiscoveryPort};
use messaging::ports::InboundEnvelopePort;

use crate::composition::{DeliveryIndex, Diagnostics, LocalEndpoints, NoticeFeed};
use crate::runtime::{delivery_failure_of, transport_reason};

/// Turns one `NetworkEvent` into the inbound port calls its documentation
/// names.
///
/// # Why this mapping is a type of its own
///
/// `NetworkEvent` says, on every variant, exactly which inbound port call it
/// becomes — and then says why it does not make that call itself:
///
/// > *an infrastructure crate that calls into two contexts' handlers from
/// > inside an async task would decide, on their behalf, which thread their
/// > aggregates are mutated on.*
///
/// So the correspondence is the composition root's to implement, which makes it
/// the root's to get wrong. Isolating it here — over the port traits, not over
/// the concrete services — is what lets every line of it be asserted against
/// fakes rather than against a live swarm.
///
/// # The two mappings that are not one-to-one
///
/// * **`SessionEstablished` becomes two calls**, `session_opened` then
///   `session_established`, in that order. The roster has a `Connecting` state
///   a session must pass through, and the endpoint the link runs on is only in
///   the first call's arguments. The event fires *only for links this peer did
///   not dial*: an outbound dial produces no event at all, because
///   `connect_to_peer` already opened and established the session on the
///   strength of `dial` returning `Ok`.
///
/// * **`EnvelopeReceived` becomes two calls**, `peer_heartbeat(from)` then
///   `accept_envelope`. `from` is the peer that handed the envelope over — the
///   requester for a direct, the propagating peer for a broadcast — and it is
///   *not* the author, which is whoever's signature verifies (invariant 4).
///   Any traffic at all is evidence of life (invariant 7), which is why a peer
///   holding a conversation needs no separate probe. The heartbeat is reported
///   first: an envelope that turns out to be from a blocked author still
///   proves its *carrier* is alive.
///
/// # What is refused rather than fatal
///
/// Every port refusal here is counted and shown, never propagated. A peer
/// reporting a heartbeat for someone the roster has never heard of, an
/// envelope the boundary refuses, a `MessageId` the index no longer holds —
/// these are the ordinary weather of an open network, and a router that
/// returned `Err` would stop draining the queue and lose everything behind it.
pub struct EventRouter {
    sessions: Arc<dyn InboundSessionPort + Send + Sync>,
    inbound: Arc<dyn InboundEnvelopePort + Send + Sync>,
    discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
    endpoints: Arc<LocalEndpoints>,
    deliveries: Arc<DeliveryIndex>,
    diagnostics: Arc<Diagnostics>,
    notices: Arc<NoticeFeed>,
}

impl EventRouter {
    pub const fn new(
        sessions: Arc<dyn InboundSessionPort + Send + Sync>,
        inbound: Arc<dyn InboundEnvelopePort + Send + Sync>,
        discovery: Arc<dyn PeerDiscoveryPort + Send + Sync>,
        endpoints: Arc<LocalEndpoints>,
        deliveries: Arc<DeliveryIndex>,
        diagnostics: Arc<Diagnostics>,
        notices: Arc<NoticeFeed>,
    ) -> Self {
        Self {
            sessions,
            inbound,
            discovery,
            endpoints,
            deliveries,
            diagnostics,
            notices,
        }
    }

    /// Routes one event.
    pub fn route(&self, event: NetworkEvent) {
        match event {
            // → nothing. The endpoints a peer announces are what
            // `PeerTransportPort::listen` returned; this is remembered only so
            // a join ticket has somewhere to point (D1).
            NetworkEvent::ListeningOn(endpoint) => {
                self.endpoints.record_listening(endpoint);
            }

            // → re-announce. This is the first moment a NAT-ed peer has a
            // truthful address to publish, and the whole set goes out, not
            // just the new one: an announcement replaces, it does not append.
            NetworkEvent::ExternalAddressConfirmed(endpoint) => {
                if self.endpoints.record_confirmed(endpoint) {
                    self.announce();
                }
            }

            // → `InboundSessionPort::peer_observed`.
            NetworkEvent::PeerDiscovered(discovered) => self.observe(discovered),

            // → `session_opened` then `session_established`.
            NetworkEvent::SessionEstablished { peer, endpoint } => {
                if let Err(error) = self.sessions.session_opened(peer, vec![endpoint]) {
                    self.refused(format_args!("session from a peer refused: {error}"));
                    return;
                }
                if let Err(error) = self.sessions.session_established(peer) {
                    self.refused(format_args!("session handshake refused: {error}"));
                }
            }

            // → `InboundSessionPort::session_closed`.
            NetworkEvent::SessionClosed { peer } => {
                if let Err(error) = self.sessions.session_closed(peer) {
                    self.refused(format_args!("session close refused: {error}"));
                }
            }

            // → `peer_heartbeat(from)` then `accept_envelope`.
            NetworkEvent::EnvelopeReceived { from, envelope } => {
                // A peer we have no roster entry for cannot be given evidence
                // of life — discovery comes first, and this is the ordinary
                // case for a gossip relay we have never dialled.
                let _ = self.sessions.peer_heartbeat(from);

                match self.inbound.accept_envelope(envelope) {
                    Ok(verdict) => self.count(&verdict),
                    Err(error) => {
                        self.refused(format_args!("an envelope could not be judged: {error}"));
                    }
                }
            }

            // → `InboundEnvelopePort::message_delivered`, by way of the
            // signature the root recorded when it handed the envelope over.
            NetworkEvent::DirectMessageDelivered { peer, signature } => {
                let Some(message) = self.deliveries.take(&signature) else {
                    // Evicted, or already answered. Counted rather than
                    // guessed at: there is no message this could name.
                    self.diagnostics.count_uncorrelated_report();
                    return;
                };

                if let Err(error) = self.inbound.message_delivered(message) {
                    self.diagnostics.count_port_refusal();
                    self.notices.warn(format!(
                        "a message to {} was acknowledged but could not be marked delivered: {error}",
                        short(peer)
                    ));
                }
            }

            // → `InboundEnvelopePort::message_delivery_failed`, by way of the
            // same signature the acknowledgement above is correlated by.
            //
            // This is the only path that can move one message off `Pending`
            // while its session is still up: `message_delivered` is the
            // opposite ending, and `peer_disconnected` fails *every* pending
            // direct to a peer, which is too much for one refused message and
            // unavailable while the link is healthy. Without it the message
            // sits `Pending` for the life of the session, which AC11 calls
            // silent loss.
            NetworkEvent::DirectMessageFailed {
                peer,
                signature,
                reason,
            } => {
                self.diagnostics.count_direct_delivery_failure();

                let Some(message) = self.deliveries.take(&signature) else {
                    // Evicted, or already answered. There is no message this
                    // could name, and failing a guessed one would mark the
                    // wrong message failed.
                    self.diagnostics.count_uncorrelated_report();
                    self.notices.warn(format!(
                        "a message to {} was not delivered: {}",
                        short(peer),
                        transport_reason(reason)
                    ));
                    return;
                };

                // The state is the domain's and the diagnosis is the
                // transport's: the conversation records one of five delivery
                // reasons, and the notice carries the sentence that explains
                // which network condition produced it.
                match self
                    .inbound
                    .message_delivery_failed(message, delivery_failure_of(reason))
                {
                    Ok(_) => self.notices.warn(format!(
                        "message {} to {} was not delivered: {}",
                        message.sequence(),
                        short(peer),
                        transport_reason(reason)
                    )),
                    // A refusal here is the conversation's ruling and is
                    // reported as it stands, never reinterpreted: a broadcast
                    // has no failed state, a message already delivered or
                    // already failed keeps what the user was shown, and an
                    // identifier no conversation holds is a typed
                    // `UnknownMessage` rather than a new conversation.
                    Err(error) => {
                        self.diagnostics.count_port_refusal();
                        self.notices.warn(format!(
                            "a message to {} was not delivered ({}), and could not be marked failed: {error}",
                            short(peer),
                            transport_reason(reason)
                        ));
                    }
                }
            }

            // → nothing. The one variant that maps onto no inbound port at
            // all, and deliberately: this is a fact about *this process's*
            // position on the network, not about a peer, a message, or a
            // session, so no context owns it (reachability canvas D5). It is
            // held for the status line and that is the entire effect it has —
            // nothing here dials, announces, reserves a relay, or changes an
            // address selection on the strength of it (D4, S5). libp2p already
            // prefers a confirmed direct address and falls back to a circuit;
            // second-guessing that from the root would duplicate the logic
            // with worse information, and would act on evidence this piece
            // trusts only far enough to *report*.
            //
            // Emitted only on a transition, so what arrives is current and
            // there is nothing here to debounce or age out.
            NetworkEvent::ReachabilityChanged(reachability) => {
                self.diagnostics.record_reachability(reachability);
            }
        }
    }

    fn observe(&self, discovered: DiscoveredPeer) {
        let peer = discovered.peer;

        if let Err(error) = self.sessions.peer_observed(discovered) {
            self.refused(format_args!(
                "a sighting of {} was refused: {error}",
                short(peer)
            ));
        }
    }

    fn announce(&self) {
        let endpoints = self.endpoints.all();

        if let Err(error) = self.discovery.announce(&endpoints) {
            self.diagnostics.count_port_refusal();
            self.notices
                .warn(format!("this peer's addresses were not announced: {error}"));
        }
    }

    fn count(&self, verdict: &messaging::ports::InboundVerdict) {
        use messaging::ports::InboundVerdict;

        match verdict {
            InboundVerdict::Ignored(_) => self.diagnostics.count_envelope_ignored(),
            // Refusals and duplicates are already counted by
            // `MessagingEventSink`, which sees the events the boundary
            // published; counting them here as well would double every number.
            InboundVerdict::RefusedAtBoundary(_) => {}
            InboundVerdict::Judged(_) => {
                if verdict.is_applied() {
                    self.diagnostics.count_envelope_accepted();
                }
            }
        }
    }

    fn refused(&self, message: std::fmt::Arguments<'_>) {
        self.diagnostics.count_port_refusal();
        self.notices.warn(message.to_string());
    }
}

impl std::fmt::Debug for EventRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventRouter").finish_non_exhaustive()
    }
}

/// A peer's first eight fingerprint characters — enough to recognise in a log
/// line, short enough to fit one.
fn short(peer: shared_types::PeerId) -> String {
    shared_types::Fingerprint::of(&peer)
        .to_string()
        .chars()
        .take(9)
        .collect()
}
