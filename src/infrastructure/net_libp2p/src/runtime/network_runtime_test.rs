use std::time::{Duration, Instant};

use membership::domain::{DurationMillis, Millis, Reachability};
use membership::ports::{PeerDiscoveryPort, PeerTransportError, PeerTransportPort};
use messaging::ports::{MessageTransportError, MessageTransportPort};
use shared_types::{Envelope, EnvelopeSignature, PayloadKind, PeerId, ProtocolVersion};

use crate::adapters::{Libp2pMessageTransport, Libp2pPeerDiscovery, Libp2pPeerTransport};
use crate::codec::CodecDiagnostics;
use crate::runtime::{NetworkConfig, NetworkIdentity, NetworkRuntime, NetworkStartError};
use crate::swarm::NetworkEvent;
use crate::test_peers::{ALICE_SECRET_KEY, BOB_SECRET_KEY, carol};
use crate::ticket::JoinTicketCodec;

/// How long the whole two-swarm exchange is given.
///
/// Generous on purpose. The assertions are on *what happened*, never on how
/// long it took: a loaded CI machine may take seconds to bind two sockets and
/// complete two handshakes, and a test that failed for that reason would be
/// telling us about the machine rather than the code. Nothing here samples a
/// wall clock or asserts an ordering that depends on one (AC13).
const DEADLINE: Duration = Duration::from_secs(30);

/// One peer, with its runtime and the three ports over it.
struct Peer {
    runtime: NetworkRuntime,
    transport: Libp2pPeerTransport,
    discovery: Libp2pPeerDiscovery,
    messages: Libp2pMessageTransport,
}

impl Peer {
    /// A peer with a **fixed** identity, bound to loopback on OS-chosen ports.
    ///
    /// Fixed keys because the collapse rule, the `PeerId` ordering, and every
    /// assertion about who is who depend on the identities; OS-chosen ports
    /// because a hardcoded one makes the test fail when anything else on the
    /// machine happens to hold it. No mDNS: a test must not multicast onto a
    /// developer's real network, and it needs no LAN rung anyway — the dial is
    /// explicit.
    fn start(secret: [u8; 32]) -> Result<Self, NetworkStartError> {
        Self::start_with(secret, &NetworkConfig::loopback())
    }

    /// The same peer, on a configuration the caller chose — the only reason
    /// being the external-address override, which is a launch-time fact and
    /// therefore cannot be applied to a peer that is already running.
    fn start_with(secret: [u8; 32], config: &NetworkConfig) -> Result<Self, NetworkStartError> {
        let mut secret = secret;
        let identity = NetworkIdentity::from_ed25519_secret_key(&mut secret)
            .expect("RFC 8032 vector is a valid secret key");
        let runtime = NetworkRuntime::start(&identity, config)?;

        Ok(Self {
            transport: runtime.peer_transport(),
            discovery: runtime.peer_discovery(),
            messages: runtime.message_transport(),
            runtime,
        })
    }

    fn peer_id(&self) -> PeerId {
        self.runtime.local_peer()
    }

    /// Waits until the counters satisfy `wanted`, or the deadline passes.
    ///
    /// Polls rather than waiting on an event, because what is being waited for
    /// here is something the driver *counted*, not something it reported. Same
    /// generous deadline and same reasoning: the assertion is on what happened,
    /// never on how long it took.
    fn await_diagnostics(&self, wanted: impl Fn(&CodecDiagnostics) -> bool) -> bool {
        let deadline = Instant::now() + DEADLINE;
        let diagnostics = self.runtime.diagnostics();

        while Instant::now() < deadline {
            if wanted(&diagnostics) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        wanted(&diagnostics)
    }

    /// Waits for the first event matching `wanted`, discarding the rest.
    fn await_event(&self, wanted: impl Fn(&NetworkEvent) -> bool) -> Option<NetworkEvent> {
        let deadline = Instant::now() + DEADLINE;
        let events = self.runtime.events();

        while Instant::now() < deadline {
            if let Some(event) = events.next_timeout(Duration::from_millis(50))
                && wanted(&event)
            {
                return Some(event);
            }
        }

        None
    }
}

fn envelope(author: PeerId, kind: PayloadKind, body: &[u8]) -> Envelope {
    let mut signature = [0_u8; EnvelopeSignature::LENGTH];
    for (index, byte) in signature.iter_mut().enumerate() {
        // A stand-in, not a real signature: verification is the identity
        // context's job and happens above this layer. What matters here is
        // that the bytes survive the round trip untouched.
        *byte = (index as u8).wrapping_add(body.len() as u8);
    }

    Envelope {
        version: ProtocolVersion::CURRENT,
        kind,
        author,
        payload: body.to_vec(),
        signature: EnvelopeSignature::new(signature),
    }
}

/// Skips the loopback tests when the environment forbids binding a socket.
///
/// A sandbox with no network namespace is a fact about the machine, not a
/// failure of this code, and a test suite that cannot tell the two apart is
/// one nobody trusts. The pure tests — codec, mapping, ticket, caps, collapse
/// — cover the logic and never touch a socket.
///
/// The skip is announced on the process's own stderr rather than swallowed by
/// the harness, and `DISTRO_REQUIRE_NETWORK_TESTS=1` turns it into a failure:
/// see [`crate::required_network`] for why a silent skip is the thing being
/// removed here.
macro_rules! peer_or_skip {
    ($secret:expr) => {
        match Peer::start($secret) {
            Ok(peer) => peer,
            Err(error) => {
                crate::required_network::skip(&error);
                return;
            }
        }
    };
    ($secret:expr, $config:expr) => {
        match Peer::start_with($secret, $config) {
            Ok(peer) => peer,
            Err(error) => {
                crate::required_network::skip(&error);
                return;
            }
        }
    };
}

#[test]
fn two_peers_on_loopback_hold_a_session_and_exchange_messages() {
    let alice = peer_or_skip!(ALICE_SECRET_KEY);
    let bob = peer_or_skip!(BOB_SECRET_KEY);

    // ---------------------------------------------------------------- listen

    let alice_endpoints = alice.transport.listen().expect("alice listens");
    let bob_endpoints = bob.transport.listen().expect("bob listens");

    assert!(!alice_endpoints.is_empty(), "listen returns what to dial");
    assert!(
        alice_endpoints
            .iter()
            .all(|endpoint| endpoint.reachability() == Reachability::Direct),
        "a loopback listener is not relayed"
    );
    assert!(!bob_endpoints.is_empty());

    // Announcing is what a peer does with those endpoints (S8).
    alice
        .discovery
        .announce(&alice_endpoints)
        .expect("alice announces");

    // ------------------------------------------------------------------ dial

    let answered = bob
        .transport
        .dial(alice.peer_id(), &alice_endpoints)
        .expect("bob reaches alice");
    assert_eq!(
        answered.reachability(),
        Reachability::Direct,
        "a loopback dial is direct, not relayed (AC12 reads this class)"
    );

    // Alice was dialled, so she — and only she — learns of it as an event.
    let established = alice
        .await_event(|event| {
            matches!(event, NetworkEvent::SessionEstablished { peer, .. } if *peer == bob.peer_id())
        })
        .expect("alice sees the inbound session");
    let NetworkEvent::SessionEstablished { endpoint, .. } = established else {
        unreachable!("matched just above")
    };
    assert_eq!(endpoint.reachability(), Reachability::Direct);

    // Dialling again is idempotent: the caller asked for a link and there is
    // one, so no second connection is opened.
    assert!(
        bob.transport
            .dial(alice.peer_id(), &alice_endpoints)
            .is_ok()
    );

    // -------------------------------------------------------- direct message

    let direct = envelope(bob.peer_id(), PayloadKind::DirectMessage, b"hello alice");
    bob.messages
        .send_direct(alice.peer_id(), &direct)
        .expect("the transport accepts it");

    let received = alice
        .await_event(|event| matches!(event, NetworkEvent::EnvelopeReceived { .. }))
        .expect("alice receives the envelope");
    let NetworkEvent::EnvelopeReceived {
        from,
        envelope: arrived,
    } = received
    else {
        unreachable!("matched just above")
    };
    assert_eq!(from, bob.peer_id(), "the peer that handed it over");
    assert_eq!(arrived, direct, "the envelope crosses the wire unchanged");
    assert_eq!(
        arrived.signable_bytes(),
        direct.signable_bytes(),
        "the signing input survives encoding and decoding"
    );

    // AC11: the sender learns the message arrived.
    let delivered = bob
        .await_event(|event| matches!(event, NetworkEvent::DirectMessageDelivered { .. }))
        .expect("bob learns it was delivered");
    assert_eq!(
        delivered,
        NetworkEvent::DirectMessageDelivered {
            peer: alice.peer_id(),
            signature: direct.signature,
        }
    );

    // ------------------------------------------------------------- broadcast

    let broadcast = envelope(
        bob.peer_id(),
        PayloadKind::BroadcastMessage,
        b"hello everybody",
    );

    // Gossip needs both ends subscribed and meshed, which happens some time
    // after the connection comes up. Re-publishing until it lands is what a
    // peer would do anyway; the assertion is that it *does* land, not when.
    let deadline = Instant::now() + DEADLINE;
    let mut arrived = None;
    while Instant::now() < deadline && arrived.is_none() {
        bob.messages
            .publish_broadcast(&broadcast)
            .expect("publishing is accepted even with nobody subscribed yet");

        let events = alice.runtime.events();
        let inner = Instant::now() + Duration::from_millis(500);
        while Instant::now() < inner {
            if let Some(NetworkEvent::EnvelopeReceived { envelope, .. }) =
                events.next_timeout(Duration::from_millis(50))
                && envelope.kind == PayloadKind::BroadcastMessage
            {
                arrived = Some(envelope);
                break;
            }
        }
    }

    assert_eq!(
        arrived.expect("alice receives the broadcast (D3, AC10)"),
        broadcast
    );

    // ----------------------------------------------------------------- close

    bob.transport
        .close_session(alice.peer_id())
        .expect("bob closes the session");
    assert_eq!(
        bob.transport.close_session(alice.peer_id()),
        Err(PeerTransportError::NoSuchSession),
        "closing a session that is already gone says so"
    );

    alice
        .await_event(
            |event| matches!(event, NetworkEvent::SessionClosed { peer } if *peer == bob.peer_id()),
        )
        .expect("alice sees the session end (AC5)");

    // Nothing was rejected, tolerated, or dropped along the way.
    let diagnostics = alice.runtime.diagnostics();
    assert_eq!(diagnostics.rejected_major(), 0);
    assert_eq!(diagnostics.malformed_frames(), 0);
    assert_eq!(diagnostics.oversize_frames(), 0);
    assert_eq!(diagnostics.rate_limited(), 0);
    assert_eq!(diagnostics.dropped_events(), 0);

    alice.runtime.shutdown();
    bob.runtime.shutdown();
}

#[test]
fn a_join_ticket_minted_from_real_endpoints_brings_a_stranger_in() {
    // AC3's third rung end to end: alice writes down where she is, bob pastes
    // the string, and the peers meet — with no operator anywhere in the path.
    let alice = peer_or_skip!(ALICE_SECRET_KEY);
    let bob = peer_or_skip!(BOB_SECRET_KEY);

    let alice_endpoints = alice.transport.listen().expect("alice listens");
    bob.transport.listen().expect("bob listens");

    let ticket = membership::domain::JoinTicket::expiring_after(
        alice.peer_id(),
        alice_endpoints.clone(),
        ProtocolVersion::CURRENT,
        Millis::ZERO,
        DurationMillis::from_secs(3_600),
    )
    .expect("a ticket with real endpoints");

    let pasted = JoinTicketCodec::encode(&ticket);
    let redeemed_ticket = JoinTicketCodec::decode(&pasted).expect("the string round-trips");
    assert_eq!(redeemed_ticket, ticket);

    let discovered = bob
        .discovery
        .redeem_join_ticket(&redeemed_ticket)
        .expect("bob reaches alice through the ticket");

    assert_eq!(discovered.peer, alice.peer_id());
    assert_eq!(discovered.endpoints, alice_endpoints);

    alice
        .await_event(|event| {
            matches!(event, NetworkEvent::SessionEstablished { peer, .. } if *peer == bob.peer_id())
        })
        .expect("alice sees the newcomer arrive");

    alice.runtime.shutdown();
    bob.runtime.shutdown();
}

#[test]
fn observed_addresses_reach_the_ledger_and_a_loopback_one_is_never_advertised() {
    // Two real swarms, so the question is whether the wiring exists at all:
    // does `identify` produce external-address candidates, does the driver's
    // new arm receive them, and does the ledger refuse the only kind of
    // address a test on one machine can ever produce?
    //
    // # What this test can and cannot prove, stated rather than implied
    //
    // Every address two peers on loopback observe each other at is
    // `127.0.0.1`, which D5 refuses. So a *promotion* cannot happen here, and
    // no arrangement of loopback peers would make one happen — the analysis
    // said as much (§5: the simulated fabric has no concept of a public
    // address). What this test proves is the half that needs two real swarms:
    // that candidates genuinely flow, that the driver sees them, and that the
    // refusal is real rather than an accident of a mocked event. The
    // promotion path itself — corroboration, `add_external_address`, and
    // `NetworkEvent::ExternalAddressConfirmed` — is proven in
    // `swarm/network_driver_test.rs`, where the observing peer can be two
    // different peers and the address can be a public one.
    let alice = peer_or_skip!(ALICE_SECRET_KEY);
    let bob = peer_or_skip!(BOB_SECRET_KEY);

    let alice_endpoints = alice.transport.listen().expect("alice listens");
    bob.transport.listen().expect("bob listens");
    bob.transport
        .dial(alice.peer_id(), &alice_endpoints)
        .expect("bob reaches alice");

    // identify runs on both sides of a new connection, so both peers learn an
    // address the other sees them at (P1-7: the counter is how anyone ever
    // finds out whether this is happening).
    assert!(
        alice.await_diagnostics(|diagnostics| diagnostics.external_candidates_seen() > 0),
        "the driver's candidate arm never received an observed address, so the \
         wiring identify has always produced is still going nowhere"
    );
    assert!(bob.await_diagnostics(|diagnostics| diagnostics.external_candidates_seen() > 0));

    for peer in [&alice, &bob] {
        let diagnostics = peer.runtime.diagnostics();
        assert_eq!(
            diagnostics.external_candidates_recorded(),
            0,
            "a loopback address is not a candidate for anything (P1-5, S3)"
        );
        assert_eq!(
            diagnostics.external_addresses_promoted(),
            0,
            "and it is certainly never advertised"
        );
    }

    alice.runtime.shutdown();
    bob.runtime.shutdown();
}

#[test]
fn a_peer_with_nowhere_to_dial_is_refused_rather_than_left_hanging() {
    // AC3's other half: failure produces a visible diagnostic, never a hang.
    let alice = peer_or_skip!(ALICE_SECRET_KEY);

    assert_eq!(
        alice.transport.dial(carol(), &[]),
        Err(PeerTransportError::NoReachableEndpoint)
    );
    assert_eq!(
        alice.transport.close_session(carol()),
        Err(PeerTransportError::NoSuchSession)
    );
    assert_eq!(
        alice.messages.send_direct(
            carol(),
            &envelope(alice.peer_id(), PayloadKind::DirectMessage, b"?")
        ),
        Err(MessageTransportError::PeerUnreachable)
    );

    alice.runtime.shutdown();
}

#[test]
fn publishing_to_a_broadcast_channel_nobody_is_listening_to_is_success() {
    // A peer alone on the network is `Isolated`, not broken — the same rule the
    // simulated fabric applies, so the two adapters agree behaviourally.
    let alice = peer_or_skip!(ALICE_SECRET_KEY);
    alice.transport.listen().expect("alice listens");

    assert_eq!(
        alice.messages.publish_broadcast(&envelope(
            alice.peer_id(),
            PayloadKind::BroadcastMessage,
            b"anyone there?"
        )),
        Ok(())
    );

    alice.runtime.shutdown();
}

#[test]
fn observing_peers_on_an_empty_network_is_success_and_not_an_error() {
    let alice = peer_or_skip!(ALICE_SECRET_KEY);

    assert_eq!(alice.discovery.observe_peers(), Ok(Vec::new()));

    alice.runtime.shutdown();
}

// --------------------------- an asserted external address (canvas `0008`)

/// The addresses these tests assert — an IPv4 and an IPv6 one, because a
/// dual-stack host that forwarded a port has both and a launch that applied
/// only the first would silently drop half of what the operator said.
///
/// RFC 5737 and RFC 3849 documentation space, so a join ticket printed by a
/// failing run names nobody's real machine.
const ASSERTED: [&str; 2] = ["/ip4/203.0.113.7/tcp/4001", "/ip6/2001:db8::7/tcp/4001"];

/// A loopback configuration that also asserts external addresses, which is the
/// whole of what `--external-address` does to a peer's launch.
fn asserting(addresses: &[&str]) -> NetworkConfig {
    NetworkConfig {
        external_addresses: addresses.iter().copied().map(str::to_owned).collect(),
        ..NetworkConfig::loopback()
    }
}

#[test]
fn a_supplied_external_address_appears_in_a_join_ticket_minted_afterwards() {
    // **P3-4, and the reason the whole piece exists.** A peer that forwarded a
    // port can hand a stranger a ticket that names the address the world
    // reaches it at — without waiting for a second peer to observe it or an
    // AutoNAT server to probe it, neither of which the first instance on a
    // network has.
    //
    // What this proves is that the assertion travels the real path: through
    // startup, into the driver's advertised set, out of `listen`, and into a
    // ticket that survives being pasted. What it cannot prove is that
    // `203.0.113.7` answers from outside — that is the operator's claim and
    // ultimately the two-machine smoke of system canvas OP-13 (`0008` S5).
    let alice = peer_or_skip!(ALICE_SECRET_KEY, &asserting(&ASSERTED));

    let endpoints = alice.transport.listen().expect("alice listens");
    for text in ASSERTED {
        let asserted = endpoints
            .iter()
            .find(|endpoint| endpoint.address() == text)
            .unwrap_or_else(|| {
                panic!("{text} is missing from the endpoints alice reports: {endpoints:?}")
            });
        assert_eq!(
            asserted.reachability(),
            Reachability::Direct,
            "an address with no circuit hop is dialled directly, whoever supplied it"
        );
    }

    let ticket = membership::domain::JoinTicket::expiring_after(
        alice.peer_id(),
        endpoints.clone(),
        ProtocolVersion::CURRENT,
        Millis::ZERO,
        DurationMillis::from_secs(3_600),
    )
    .expect("a ticket with real endpoints");

    let pasted = JoinTicketCodec::encode(&ticket);
    let decoded = JoinTicketCodec::decode(&pasted).expect("the string round-trips");

    // Each assertion was also reported over the same event pieces 1 and 2 use,
    // not a second pipe (P3-2). Drained once and checked as a set, so the test
    // does not depend on the order two confirmations happen to arrive in.
    let confirmed: Vec<String> = alice
        .runtime
        .events()
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            NetworkEvent::ExternalAddressConfirmed(endpoint) => Some(endpoint.address().to_owned()),
            _ => None,
        })
        .collect();

    for text in ASSERTED {
        assert!(
            decoded
                .endpoints()
                .iter()
                .any(|endpoint| endpoint.address() == text),
            "{text} must survive into the string a human pastes (P3-4)"
        );
        assert!(
            confirmed.iter().any(|address| address == text),
            "the root was never told about {text} over the confirmation path; \
             it saw {confirmed:?}"
        );
    }

    // S1: it is *our* address. Nothing turned it into a peer to contact.
    assert_eq!(
        alice.discovery.observe_peers(),
        Ok(Vec::new()),
        "an asserted address is advertised, never dialled and never cached as \
         a peer"
    );

    alice.runtime.shutdown();
}

#[test]
fn a_non_global_external_address_refuses_the_start_rather_than_being_ignored() {
    // P3-8/S3 at the surface a user meets. The exhaustive table of refused
    // classes is asserted in `swarm/network_driver_test.rs`, against the same
    // predicate; what this adds is that the refusal reaches the caller of
    // `start` instead of being swallowed into a peer that launched and quietly
    // advertised nothing.
    match Peer::start_with(
        ALICE_SECRET_KEY,
        &asserting(&["/ip4/192.168.1.20/tcp/4001"]),
    ) {
        Err(NetworkStartError::NonGlobalExternalAddress) => {}
        // The refusal happens after the transports are assembled, so a machine
        // that cannot assemble them reports its own problem first. That is a
        // fact about the machine, and it skips loudly rather than passing
        // silently — see `crate::required_network`.
        Err(other) => crate::required_network::skip(&other),
        Ok(_) => panic!(
            "a private address was accepted as an external one; advertising \
             192.168.x.x globally is never useful (P3-8)"
        ),
    }
}

#[test]
fn a_malformed_external_address_refuses_the_start_before_anything_is_built() {
    // S4/invariant 4 at this layer. `app` validates the syntax of what it was
    // typed, but this crate takes a `NetworkConfig` from whoever hands it one
    // and must not advertise a string it cannot parse — nor start a peer that
    // silently advertises nothing. No socket is involved: the refusal happens
    // before a runtime exists, so this test is deterministic everywhere.
    assert_eq!(
        Peer::start_with(ALICE_SECRET_KEY, &asserting(&["not-a-multiaddress"]))
            .map(|_| ())
            .unwrap_err(),
        NetworkStartError::MalformedExternalAddress
    );
}

#[test]
fn the_non_global_refusal_says_mdns_already_covers_the_local_network() {
    // The message is the feature here. A user who typed their LAN address into
    // `--external-address` was trying to be reachable from another machine, and
    // the useful answer is not "rejected" — it is that this build already finds
    // peers on the local link without being told anything at all, so the flag
    // they reached for is one they do not need.
    let message = NetworkStartError::NonGlobalExternalAddress.to_string();

    assert!(
        message.contains("mDNS"),
        "the refusal must name the mechanism that already covers this case: {message}"
    );
    assert!(
        message.contains("local network"),
        "and say what that mechanism covers: {message}"
    );
}

#[test]
fn every_port_refuses_rather_than_hangs_once_the_runtime_is_gone() {
    // Point 5 of the runtime contract: the adapters outlive the runtime, and
    // what they do afterwards is refuse — never block a caller forever.
    let alice = peer_or_skip!(ALICE_SECRET_KEY);
    let transport = alice.transport.clone();
    let discovery = alice.discovery.clone();
    let messages = alice.messages.clone();
    let identity = alice.peer_id();

    alice.runtime.shutdown();

    assert_eq!(transport.listen(), Err(PeerTransportError::ListenFailed));
    assert_eq!(
        transport.close_session(carol()),
        Err(PeerTransportError::Unavailable)
    );
    assert!(discovery.observe_peers().is_err());
    assert_eq!(
        messages.publish_broadcast(&envelope(
            identity,
            PayloadKind::BroadcastMessage,
            b"into the void"
        )),
        Err(MessageTransportError::Unavailable)
    );
}
