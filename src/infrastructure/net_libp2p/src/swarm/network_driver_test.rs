//! What the driver does with an external-address candidate.
//!
//! # Why this test drives the driver rather than two real swarms
//!
//! The rule under test is a *security* rule (S4): a candidate counts toward
//! corroboration only when it can be attributed to the peer whose identify
//! exchange produced it. Two real swarms cannot demonstrate that. They can only
//! be given loopback addresses, which the ledger refuses before attribution is
//! ever consulted, so a run in which attribution was silently broken would look
//! exactly like a run in which it worked.
//!
//! Feeding the driver the swarm events libp2p would have handed it — in the
//! order libp2p guarantees — is what makes the rule observable. The swarm is
//! real and built by the same `build_swarm` production uses; only the events
//! are supplied, and every one of them is a shape identify actually emits.

use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

use libp2p::gossipsub::IdentTopic;
use libp2p::identity::Keypair;
use libp2p::swarm::{ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId as Libp2pPeerId, identify};
use membership::domain::Reachability;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::runtime::network_runtime::build_swarm;
use crate::runtime::{NetworkConfig, NetworkIdentity};
use crate::swarm::NetworkEvent;
use crate::swarm::distro_behaviour::DistroBehaviourEvent;
use crate::swarm::network_command::NetworkCommand;
use crate::swarm::network_driver::NetworkDriver;
use crate::test_peers::ALICE_SECRET_KEY;

/// A globally routable address, from RFC 5737's documentation range so nothing
/// here can be mistaken for a real host.
const PUBLIC: &str = "/ip4/203.0.113.7/tcp/4001";

/// The driver, the queue it writes to, and the counters it increments.
struct Harness {
    /// Held for its lifetime: the transports registered with this reactor when
    /// the swarm was built.
    _runtime: tokio::runtime::Runtime,
    /// Held open so the driver's command channel never reads as closed.
    _commands: UnboundedSender<NetworkCommand>,
    driver: NetworkDriver,
    events: Receiver<NetworkEvent>,
    diagnostics: CodecDiagnostics,
}

impl Harness {
    /// A driver over a real swarm that is listening on nothing.
    ///
    /// `None` when the machine will not give the transports a reactor, which is
    /// a fact about the machine rather than a failure of this code — the same
    /// treatment the loopback tests give a socket they cannot bind.
    fn start() -> Option<Self> {
        let mut secret = ALICE_SECRET_KEY;
        let identity = NetworkIdentity::from_ed25519_secret_key(&mut secret)
            .expect("RFC 8032 vector is a valid secret key");
        let config = NetworkConfig::loopback();
        let topic = IdentTopic::new(&config.broadcast_topic);

        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                crate::required_network::skip(&error);
                return None;
            }
        };

        let swarm = {
            let _guard = runtime.enter();
            match build_swarm(&identity, &config, &topic) {
                Ok(swarm) => swarm,
                Err(error) => {
                    crate::required_network::skip(&error);
                    return None;
                }
            }
        };

        let diagnostics = CodecDiagnostics::new();
        let codec = EnvelopeCodec::new(config.protocol_version, config.limits, diagnostics.clone());
        let (commands_tx, commands_rx) = unbounded_channel();
        let (events_tx, events_rx) = sync_channel(config.limits.event_queue_capacity);

        Some(Self {
            driver: NetworkDriver::new(
                swarm,
                identity.peer_id(),
                topic,
                Vec::new(),
                config.limits,
                codec,
                diagnostics.clone(),
                commands_rx,
                events_tx,
            ),
            _runtime: runtime,
            _commands: commands_tx,
            events: events_rx,
            diagnostics,
        })
    }

    /// Hands the driver the identify report a peer just made about us, then the
    /// candidate identify derived from it — the two events, in the order the
    /// swarm delivers them.
    fn identify_then_candidate(&mut self, observer: &Keypair, candidate: &str) {
        self.driver.handle_swarm_event(identified(observer));
        self.driver
            .handle_swarm_event(SwarmEvent::NewExternalAddrCandidate {
                address: address(candidate),
            });
    }

    /// Every event the driver has pushed so far.
    fn drain(&self) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return events,
            }
        }
    }
}

macro_rules! harness_or_skip {
    () => {
        match Harness::start() {
            Some(harness) => harness,
            None => return,
        }
    };
}

fn address(text: &str) -> Multiaddr {
    text.parse().expect("a well-formed multiaddress")
}

fn keypair(seed: u8) -> Keypair {
    Keypair::ed25519_from_bytes([seed; 32]).expect("32 bytes are a valid Ed25519 seed")
}

/// `identify::Event::Received` exactly as the behaviour emits it.
///
/// `listen_addrs` is empty on purpose: peer discovery is a separate path with
/// its own tests, and leaving it empty keeps this test about one thing.
/// `observed_addr` is populated because identify always populates it — and is
/// deliberately *different* from the candidate the driver is given next, so a
/// driver that quietly read it instead of the candidate would fail here.
fn identified(observer: &Keypair) -> SwarmEvent<DistroBehaviourEvent> {
    SwarmEvent::Behaviour(DistroBehaviourEvent::Identify(identify::Event::Received {
        connection_id: ConnectionId::new_unchecked(1),
        peer_id: observer.public().to_peer_id(),
        info: identify::Info {
            public_key: observer.public(),
            protocol_version: "/distro/id/1.0.0".to_owned(),
            agent_version: "distro-test".to_owned(),
            listen_addrs: Vec::new(),
            protocols: Vec::new(),
            observed_addr: address("/ip4/198.51.100.9/tcp/9999"),
            signed_peer_record: None,
        },
    }))
}

fn confirmed_addresses(events: &[NetworkEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            NetworkEvent::ExternalAddressConfirmed(endpoint) => Some(endpoint.address().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn two_distinct_peers_reporting_one_public_address_confirm_it() {
    // P1-2 and P1-3 end to end through the driver: the second distinct
    // observer promotes, and promotion enters the confirmation path that the
    // composition root already listens on.
    let mut harness = harness_or_skip!();

    harness.identify_then_candidate(&keypair(1), PUBLIC);
    assert_eq!(harness.diagnostics.external_candidates_seen(), 1);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 1);
    assert_eq!(
        harness.diagnostics.external_addresses_promoted(),
        0,
        "one peer's word is never enough (S2)"
    );
    assert!(
        confirmed_addresses(&harness.drain()).is_empty(),
        "nothing is advertised on a single observation (P1-1)"
    );

    harness.identify_then_candidate(&keypair(2), PUBLIC);
    assert_eq!(harness.diagnostics.external_candidates_seen(), 2);
    assert_eq!(
        harness.diagnostics.external_candidates_recorded(),
        1,
        "the promoting observation is counted as a promotion, not as a record"
    );
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 1);

    let events = harness.drain();
    assert_eq!(
        confirmed_addresses(&events),
        vec![PUBLIC.to_owned()],
        "the corroborated address reaches `NetworkEvent::ExternalAddressConfirmed` (D4)"
    );
    let Some(NetworkEvent::ExternalAddressConfirmed(endpoint)) = events
        .into_iter()
        .find(|event| matches!(event, NetworkEvent::ExternalAddressConfirmed(_)))
    else {
        unreachable!("asserted just above")
    };
    assert_eq!(
        endpoint.reachability(),
        Reachability::Direct,
        "a corroborated address is one a stranger dials directly"
    );

    // A third observer changes nothing: an address is promoted at most once
    // (invariant 1).
    harness.identify_then_candidate(&keypair(3), PUBLIC);
    assert_eq!(harness.diagnostics.external_candidates_seen(), 3);
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 1);
    assert!(confirmed_addresses(&harness.drain()).is_empty());
}

#[test]
fn one_peer_reporting_the_same_address_repeatedly_never_confirms_it() {
    // S2 at the driver level. The counters are the visible half: every
    // observation is seen and recorded, and none of them promotes.
    let mut harness = harness_or_skip!();
    let liar = keypair(1);

    for _ in 0..8 {
        harness.identify_then_candidate(&liar, PUBLIC);
    }

    assert_eq!(harness.diagnostics.external_candidates_seen(), 8);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 8);
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
    assert!(confirmed_addresses(&harness.drain()).is_empty());
}

#[test]
fn a_candidate_with_no_identify_before_it_is_seen_and_never_counted() {
    // S4, stated as a test. A candidate that arrives outside the attribution
    // window belongs to nobody, and an observation attributed to nobody must
    // not count toward a threshold that means "distinct peers agreed".
    let mut harness = harness_or_skip!();

    harness
        .driver
        .handle_swarm_event(SwarmEvent::NewExternalAddrCandidate {
            address: address(PUBLIC),
        });

    assert_eq!(harness.diagnostics.external_candidates_seen(), 1);
    assert_eq!(
        harness.diagnostics.external_candidates_recorded(),
        0,
        "an unattributed observation is visible in diagnostics and nowhere else"
    );
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
}

#[test]
fn an_event_between_identify_and_a_candidate_closes_the_attribution_window() {
    // The window is exactly one event wide. Anything else in between means the
    // candidate did not come from that identify exchange, and guessing which
    // peer it belonged to would be the same unattributed count S4 forbids —
    // only harder to notice.
    let mut harness = harness_or_skip!();

    harness.driver.handle_swarm_event(identified(&keypair(1)));
    harness.driver.handle_swarm_event(SwarmEvent::Dialing {
        peer_id: None,
        connection_id: ConnectionId::new_unchecked(7),
    });
    harness
        .driver
        .handle_swarm_event(SwarmEvent::NewExternalAddrCandidate {
            address: address(PUBLIC),
        });

    assert_eq!(harness.diagnostics.external_candidates_seen(), 1);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 0);
}

#[test]
fn every_candidate_of_one_identify_exchange_shares_that_exchange_s_observer() {
    // identify emits one candidate per translated address from a single
    // `Received`, so the window has to survive a candidate rather than being
    // consumed by the first one — otherwise the second translated address of
    // every exchange would go uncounted.
    let mut harness = harness_or_skip!();

    harness.driver.handle_swarm_event(identified(&keypair(1)));
    for port in [4001, 4002, 4003] {
        harness
            .driver
            .handle_swarm_event(SwarmEvent::NewExternalAddrCandidate {
                address: address(&format!("/ip4/203.0.113.7/tcp/{port}")),
            });
    }

    assert_eq!(harness.diagnostics.external_candidates_seen(), 3);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 3);
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
}

#[test]
fn a_private_address_two_peers_agree_on_is_never_confirmed() {
    // P1-5/S3 through the driver: two peers on one LAN both observe each other
    // at a private address and agree perfectly. Agreement is not the question.
    let mut harness = harness_or_skip!();

    harness.identify_then_candidate(&keypair(1), "/ip4/192.168.1.20/tcp/4001");
    harness.identify_then_candidate(&keypair(2), "/ip4/192.168.1.20/tcp/4001");

    assert_eq!(harness.diagnostics.external_candidates_seen(), 2);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 0);
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
    assert!(confirmed_addresses(&harness.drain()).is_empty());
}

#[test]
fn a_peer_that_reports_our_own_identity_as_the_observer_corroborates_nothing() {
    // A connection claiming this peer's own identity is already refused at the
    // link registry, but the ledger refuses it again on its own account: the
    // corroboration rule counts *other* peers, and it must not depend on
    // another component having caught the impostor first.
    let mut harness = harness_or_skip!();

    let mut secret = ALICE_SECRET_KEY;
    let ourselves = Keypair::ed25519_from_bytes(&mut secret).expect("the fixture is a valid key");

    harness.identify_then_candidate(&ourselves, PUBLIC);
    harness.identify_then_candidate(&keypair(1), PUBLIC);

    assert_eq!(harness.diagnostics.external_candidates_seen(), 2);
    assert_eq!(
        harness.diagnostics.external_candidates_recorded(),
        1,
        "only the real observer was counted"
    );
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
}

#[test]
fn the_candidate_arm_never_reads_the_observed_address_identify_reported() {
    // D1: the address comes from the candidate event, which identify produced
    // *after* NAT address translation; `info.observed_addr` is the untranslated
    // input to that and must not be read here. The fixture reports one address
    // and emits a different one as the candidate, so a driver that read the
    // wrong field would confirm the wrong address.
    let mut harness = harness_or_skip!();

    harness.identify_then_candidate(&keypair(1), PUBLIC);
    harness.identify_then_candidate(&keypair(2), PUBLIC);

    assert_eq!(
        confirmed_addresses(&harness.drain()),
        vec![PUBLIC.to_owned()]
    );
}

/// The observer this crate maps a keypair to, used only to keep the fixtures
/// honest: two different seeds must be two different peers, or the
/// distinct-observer tests would prove nothing.
#[test]
fn the_test_fixtures_produce_distinct_observers() {
    let observers: Vec<Libp2pPeerId> = (1..=3)
        .map(|seed| keypair(seed).public().to_peer_id())
        .collect();

    assert_ne!(observers[0], observers[1]);
    assert_ne!(observers[1], observers[2]);
    assert_ne!(observers[0], observers[2]);
}
