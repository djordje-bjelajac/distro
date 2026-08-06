//! What the driver does with an external-address candidate, with an AutoNAT
//! probe report, and with an address the operator simply asserts.
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
//!
//! # The same applies, harder, to the AutoNAT verdict
//!
//! The reachability tests below share this file for the same reason and one
//! more: **no automated test anywhere can prove real unreachability** (canvas
//! S4). A loopback pair has no NAT to fail behind, and `infra-sim-net` has no
//! concept of a public address. Worse, `autonat::v2::client::Error` has a
//! private field and no public constructor, so a *failing* `client::Event`
//! cannot be built here at all. What follows proves the logic and the wiring —
//! that a supplied success reaches `NetworkEvent::ReachabilityChanged`, and
//! that failures are corroborated before they condemn. It does not prove that a
//! genuinely unreachable peer says so; that is the two-machine smoke of system
//! canvas OP-13, which has not been run.
//!
//! # And the same again for an asserted address
//!
//! The third source of an advertised address — `--external-address`, canvas
//! `0008` — is an operator's claim, so what can be tested is what this process
//! *does* with the claim: that a global one reaches the same
//! `NetworkEvent::ExternalAddressConfirmed` a corroborated one does (P3-2),
//! that a non-global one is refused (P3-8), and — the assertions that matter
//! most, because this is the one place the three pieces could contradict each
//! other — that supplying one silences neither the ledger above nor the probes
//! above that (P3-7/S2). Whether the asserted address genuinely works from
//! outside is the operator's claim, and nothing here pretends to check it
//! (`0008` S5).
//!
//! # And once more for the two adapter defects of canvas `0010`
//!
//! The same harness, for the same reason, extended rather than duplicated. A
//! destructive read (D12) and a broadcast that reached nobody (D11) are both
//! states two real loopback swarms cannot be *held in*: a loopback pair
//! discovers each other and meshes, so the failing case never arises. Supplying
//! the mDNS event libp2p would have delivered, and then asking the driver
//! twice, is what makes "the second join has the same rung available to it as
//! the first" an assertion rather than a hope.
//!
//! What is asserted below is the empty half of D11 — a publish that found
//! nobody. The propagated half needs a second peer actually subscribed to the
//! topic, so it lives in `runtime/network_runtime_test.rs` beside the two-swarm
//! broadcast that is already there; the pair of them is what makes the two
//! outcomes *distinguishable* rather than merely counted.

use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

use libp2p::gossipsub::IdentTopic;
use libp2p::identity::Keypair;
use libp2p::swarm::{ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId as Libp2pPeerId, autonat, identify, mdns};
use membership::ports::DiscoveredPeer;
// Two different questions share this name: the domain's is a property of one
// *address* (direct or relayed), this crate's is a property of *this peer's*
// position on the network. Aliased rather than qualified so neither reads as
// the other.
use membership::domain::Reachability as EndpointReachability;
use messaging::ports::MessageTransportError;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::limits::ResourceLimits;
use crate::mapping::EndpointMapping;
use crate::runtime::network_runtime::build_swarm;
use crate::runtime::{NetworkConfig, NetworkIdentity, NetworkStartError};
use crate::swarm::NetworkEvent;
use crate::swarm::distro_behaviour::DistroBehaviourEvent;
use crate::swarm::external_address_ledger::NON_GLOBAL;
use crate::swarm::network_command::NetworkCommand;
use crate::swarm::network_driver::NetworkDriver;
use crate::swarm::reachability_ledger::{ProbeResult, Reachability};
use crate::test_peers::ALICE_SECRET_KEY;

/// A globally routable address, from RFC 5737's documentation range so nothing
/// here can be mistaken for a real host.
const PUBLIC: &str = "/ip4/203.0.113.7/tcp/4001";

/// A second globally routable address, so a test can tell an *asserted* address
/// apart from an *observed* one rather than watching them collide.
const OTHER_PUBLIC: &str = "/ip4/203.0.113.8/tcp/4001";

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
    /// The caps this driver was built with, so a test asserts against the
    /// numbers in force rather than against a copy of them.
    limits: ResourceLimits,
}

impl Harness {
    /// A driver over a real swarm that is listening on nothing.
    ///
    /// `None` when the machine will not give the transports a reactor, which is
    /// a fact about the machine rather than a failure of this code — the same
    /// treatment the loopback tests give a socket they cannot bind.
    fn start() -> Option<Self> {
        Self::start_with(ResourceLimits::DEFAULT)
    }

    /// The same driver with one limit moved, so a bound can be reached by a
    /// test instead of by an attacker.
    ///
    /// The same device `ResourceLimits` documents itself: the shipped values
    /// are the ones production reads, and a test drives one of them down to
    /// something it can actually exhaust.
    fn start_with(limits: ResourceLimits) -> Option<Self> {
        let mut secret = ALICE_SECRET_KEY;
        let identity = NetworkIdentity::from_ed25519_secret_key(&mut secret)
            .expect("RFC 8032 vector is a valid secret key");
        let config = NetworkConfig {
            limits,
            ..NetworkConfig::loopback()
        };
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
            limits: config.limits,
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

    /// Asserts one external address at the driver, the way startup does.
    ///
    /// The same call `NetworkRuntime::start` makes, with the same argument —
    /// there is no test-only entry point, so what is exercised below is the
    /// production path and not a rehearsal of it.
    fn assert_external(&mut self, text: &str) -> Result<(), NetworkStartError> {
        self.driver.assert_external_address(address(text))
    }

    /// Hands the driver a successful AutoNAT probe report, in the shape
    /// `autonat::v2::client` emits it — a real `DistroBehaviourEvent` through
    /// the real `handle_swarm_event`, so the match arm is what routes it.
    fn probe_succeeded(&mut self, server: &Keypair, tested: &str) {
        self.driver
            .handle_swarm_event(SwarmEvent::Behaviour(DistroBehaviourEvent::AutonatClient(
                autonat::v2::client::Event {
                    tested_addr: address(tested),
                    // Whatever the server had to send to run the test. Nothing
                    // reads it, and it is populated to keep the fixture the
                    // shape libp2p actually produces.
                    bytes_sent: 30_000,
                    server: server.public().to_peer_id(),
                    result: Ok(()),
                },
            )));
    }

    /// Hands the driver a failed AutoNAT probe report.
    ///
    /// Not a supplied `SwarmEvent`, and it cannot be one:
    /// `autonat::v2::client::Error` has a private field and no constructor, so
    /// `Err(..)` is unconstructible outside `libp2p-autonat`. This enters at the
    /// method the match arm calls, one step further in.
    fn probe_failed(&mut self, server: &Keypair, tested: &str) {
        self.driver.probe_reported(
            server.public().to_peer_id(),
            &address(tested),
            ProbeResult::Failed,
        );
    }

    /// Hands the driver an mDNS sighting, in the shape the behaviour emits it.
    ///
    /// A real `DistroBehaviourEvent` through the real `handle_swarm_event`, so
    /// the match arm is what routes it — mDNS is off in every test
    /// configuration (`NetworkConfig::loopback`), and it is the rung D12's
    /// defect broke, so supplying the event is the only way to exercise it.
    fn mdns_discovered(&mut self, sightings: Vec<(Libp2pPeerId, Multiaddr)>) {
        self.driver
            .handle_swarm_event(SwarmEvent::Behaviour(DistroBehaviourEvent::Mdns(
                mdns::Event::Discovered(sightings),
            )));
    }

    /// Asks the driver what discovery has seen, over the real command arm.
    ///
    /// Not a test-only accessor on the driver: the defect was *in the arm*
    /// (`std::mem::take`), so a test that reached past it would have passed
    /// against the broken build.
    fn observe_peers(&mut self) -> Vec<DiscoveredPeer> {
        let (reply, answer) = sync_channel(1);
        self.driver
            .handle_command(NetworkCommand::ObservePeers { reply });
        answer
            .try_recv()
            .expect("the driver answers an observation immediately")
            .expect("observing is never an error here")
    }

    /// Publishes one frame to the broadcast topic, over the real command arm.
    fn publish_broadcast(&mut self, frame: &[u8]) -> Result<(), MessageTransportError> {
        let (reply, answer) = sync_channel(1);
        self.driver
            .handle_command(NetworkCommand::PublishBroadcast {
                frame: frame.to_vec(),
                reply,
            });
        answer
            .try_recv()
            .expect("the driver answers a publish immediately")
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
    identified_listening_at(observer, Vec::new())
}

/// The same event, with addresses the observer says *it* can be reached at —
/// which is what makes the driver remember a peer worth dialling.
fn identified_listening_at(
    observer: &Keypair,
    listen_addrs: Vec<Multiaddr>,
) -> SwarmEvent<DistroBehaviourEvent> {
    SwarmEvent::Behaviour(DistroBehaviourEvent::Identify(identify::Event::Received {
        connection_id: ConnectionId::new_unchecked(1),
        peer_id: observer.public().to_peer_id(),
        info: identify::Info {
            public_key: observer.public(),
            protocol_version: "/distro/id/1.0.0".to_owned(),
            agent_version: "distro-test".to_owned(),
            listen_addrs,
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
        EndpointReachability::Direct,
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

/// The verdict a peer proven reachable at `text` should report.
fn reachable_at(text: &str) -> Reachability {
    Reachability::Reachable(EndpointMapping::parse(text).expect("a well-formed multiaddress"))
}

/// Every reachability verdict the driver has pushed, in order.
fn verdicts(events: &[NetworkEvent]) -> Vec<Reachability> {
    events
        .iter()
        .filter_map(|event| match event {
            NetworkEvent::ReachabilityChanged(reachability) => Some(reachability.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_successful_probe_reaches_the_composition_root_as_reachable() {
    // P2-1 and P2-5 through the arm this piece adds: a real
    // `DistroBehaviourEvent::AutonatClient` carrying `Ok(())`, delivered through
    // the same `handle_swarm_event` libp2p calls, arrives at the root as a
    // verdict naming the address that was tested.
    let mut harness = harness_or_skip!();

    harness.probe_succeeded(&keypair(1), PUBLIC);

    let events = harness.drain();
    assert_eq!(verdicts(&events), vec![reachable_at(PUBLIC)]);
    assert!(
        confirmed_addresses(&events).is_empty(),
        "the confirmation path is libp2p's own `ExternalAddrConfirmed`, not this arm (D1)"
    );
}

#[test]
fn a_single_failed_probe_reports_nothing_at_all() {
    // **The asymmetry at the driver level (S2), and the point of the piece.**
    // The failure is counted, so it is not invisible — and it produces no
    // verdict, so a user is never told they are unreachable on one server's
    // word.
    let mut harness = harness_or_skip!();

    harness.probe_failed(&keypair(1), PUBLIC);

    assert_eq!(harness.diagnostics.probes_run(), 1);
    assert_eq!(harness.diagnostics.probes_failed(), 1);
    assert!(
        verdicts(&harness.drain()).is_empty(),
        "still Unknown, and Unknown is reported by saying nothing (S3)"
    );
}

#[test]
fn two_distinct_servers_failing_report_unreachable_exactly_once() {
    // P2-2 and P2-4 through the driver. The second distinct server is what
    // turns evidence into a verdict, and further failures add nothing — the
    // root is not woken by a state it already holds.
    let mut harness = harness_or_skip!();

    harness.probe_failed(&keypair(1), PUBLIC);
    assert!(verdicts(&harness.drain()).is_empty());

    harness.probe_failed(&keypair(2), PUBLIC);
    assert_eq!(
        verdicts(&harness.drain()),
        vec![Reachability::Unreachable],
        "two distinct servers agreeing is the bar (D2)"
    );

    for seed in 3..=8 {
        harness.probe_failed(&keypair(seed), PUBLIC);
    }
    assert!(verdicts(&harness.drain()).is_empty());
}

#[test]
fn one_server_failing_repeatedly_never_reports_unreachable() {
    // A broken, overloaded, or hostile server, asked eight times. Corroboration
    // counts distinct servers; if it counted reports, this peer would condemn
    // itself on one peer's say-so.
    let mut harness = harness_or_skip!();

    for _ in 0..8 {
        harness.probe_failed(&keypair(1), PUBLIC);
    }

    assert_eq!(harness.diagnostics.probes_failed(), 8);
    assert!(verdicts(&harness.drain()).is_empty());
}

#[test]
fn a_success_after_unreachable_reports_reachable_again() {
    // P2-7 at the driver level: the verdict is not a one-way latch, and the
    // return trip reaches the root over the same channel.
    let mut harness = harness_or_skip!();
    harness.probe_failed(&keypair(1), PUBLIC);
    harness.probe_failed(&keypair(2), PUBLIC);
    assert_eq!(verdicts(&harness.drain()), vec![Reachability::Unreachable]);

    harness.probe_succeeded(&keypair(3), PUBLIC);

    assert_eq!(verdicts(&harness.drain()), vec![reachable_at(PUBLIC)]);
}

#[test]
fn every_probe_is_counted_exactly_once_and_the_totals_agree() {
    // P2-6. Reachability's failure mode is silence, so the counters matter as
    // much as the state: `run` is the number of probes another peer reported on,
    // and it must equal the successes plus the failures or one of the three is
    // being incremented on the wrong path.
    let mut harness = harness_or_skip!();

    harness.probe_succeeded(&keypair(1), PUBLIC);
    harness.probe_failed(&keypair(2), PUBLIC);
    harness.probe_failed(&keypair(3), PUBLIC);
    harness.probe_succeeded(&keypair(4), PUBLIC);

    assert_eq!(harness.diagnostics.probes_run(), 4);
    assert_eq!(harness.diagnostics.probes_succeeded(), 2);
    assert_eq!(harness.diagnostics.probes_failed(), 2);
    assert_eq!(
        harness.diagnostics.probes_run(),
        harness.diagnostics.probes_succeeded() + harness.diagnostics.probes_failed()
    );
}

#[test]
fn a_probe_report_does_not_disturb_the_external_address_ledger() {
    // S5 and the piece-1 boundary: this arm reports, and changes nothing about
    // which addresses are advertised. A probe arriving between an identify
    // exchange and its candidate also closes the attribution window, exactly as
    // any other swarm event does — it is not special-cased into it.
    let mut harness = harness_or_skip!();

    harness.identify_then_candidate(&keypair(1), PUBLIC);
    harness.probe_succeeded(&keypair(2), PUBLIC);

    assert_eq!(harness.diagnostics.external_candidates_seen(), 1);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 1);
    assert_eq!(
        harness.diagnostics.external_addresses_promoted(),
        0,
        "a probe is not an observation, and does not corroborate one"
    );
    assert!(confirmed_addresses(&harness.drain()).is_empty());
}

// ------------------------------------------------ an asserted address (0008)

/// The event the composition root should see for an address advertised at
/// `text`, whichever of the three sources produced it.
///
/// Built through [`EndpointMapping`] rather than by hand so that the
/// reachability class is derived from the address the same way production
/// derives it — an asserted address is `Direct` because it contains no circuit
/// hop, not because a test said so.
fn confirmed_at(text: &str) -> NetworkEvent {
    NetworkEvent::ExternalAddressConfirmed(
        EndpointMapping::parse(text).expect("a well-formed multiaddress"),
    )
}

/// A relay circuit address through `PUBLIC`: reachable *through* another peer,
/// which is not something this peer may assert about itself.
fn circuit() -> String {
    format!(
        "{PUBLIC}/p2p/{}/p2p-circuit",
        keypair(9).public().to_peer_id()
    )
}

#[test]
fn a_supplied_external_address_reaches_the_confirmation_path_the_other_two_use() {
    // P3-1 and P3-2. The whole of the feature at this level: the operator's
    // claim arrives at the composition root as the same event a corroborated
    // observation and a successful probe arrive as, so announcements, DHT
    // records, and join tickets follow with no new code (D2).
    let mut harness = harness_or_skip!();

    assert_eq!(harness.assert_external(PUBLIC), Ok(()));

    assert_eq!(
        harness.drain(),
        vec![confirmed_at(PUBLIC)],
        "one advertisement, over the existing path, and nothing else"
    );
    assert_eq!(
        harness.diagnostics.external_candidates_seen(),
        0,
        "an assertion is not an observation and must not be counted as one"
    );
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 0);
}

#[test]
fn several_supplied_external_addresses_are_every_one_advertised() {
    // P3-1's repeatability at the level that decides it. A dual-stack host has
    // an IPv4 and an IPv6 external address, and a host that forwarded both
    // transports has two of each; taking only the first would silently drop
    // half of what the operator asserted.
    let mut harness = harness_or_skip!();
    let supplied = [
        PUBLIC,
        OTHER_PUBLIC,
        "/ip6/2001:db8::1/tcp/4001",
        "/ip4/203.0.113.7/udp/4001/quic-v1",
    ];

    for text in supplied {
        assert_eq!(harness.assert_external(text), Ok(()), "{text}");
    }

    assert_eq!(
        harness.drain(),
        supplied
            .iter()
            .copied()
            .map(confirmed_at)
            .collect::<Vec<_>>(),
        "every supplied address is advertised, in the order it was supplied"
    );
}

#[test]
fn no_non_global_supplied_address_is_ever_advertised() {
    // P3-8/S3, against the *same* table piece 1's ledger is asserted against
    // (`NON_GLOBAL` lives beside the predicate for exactly this reason). A
    // second filter written here would agree with that one today and drift from
    // it on the first class either side remembered alone.
    //
    // Refused rather than warned: this build clears the screen for a TUI
    // moments later, so a warning is a message to nobody.
    let mut harness = harness_or_skip!();

    for (text, why) in NON_GLOBAL {
        assert_eq!(
            harness.assert_external(text),
            Err(NetworkStartError::NonGlobalExternalAddress),
            "{text} ({why}) must never be advertised"
        );
    }

    assert_eq!(
        harness.assert_external(&circuit()),
        Err(NetworkStartError::NonGlobalExternalAddress),
        "a circuit's public relay address is the relay's, not ours"
    );

    assert!(
        harness.drain().is_empty(),
        "a refused address reaches nothing at all — not the event queue, not \
         the swarm, not a join ticket"
    );
}

#[test]
fn an_asserted_address_is_advertised_and_never_becomes_a_peer_to_contact() {
    // **S1, the safeguard this option exists in tension with.** The value is
    // this peer's *own* address. It is advertised so strangers can reach us,
    // and it is never dialled, cached as a peer, or handed to Kademlia as
    // somebody's address — which is the entire distinction between this option
    // and the bootstrap list this project does not have.
    let mut harness = harness_or_skip!();

    assert_eq!(harness.assert_external(PUBLIC), Ok(()));

    assert_eq!(
        harness.drain(),
        vec![confirmed_at(PUBLIC)],
        "advertised — and no peer was discovered, because there was no peer"
    );
    assert_eq!(
        harness.driver.known_peer_address_count(),
        0,
        "an asserted address must not enter the set of addresses this peer \
         would dial (S1)"
    );

    // The control, so the zero above is a fact rather than a broken accessor:
    // an address that genuinely belongs to somebody else does land there.
    harness.driver.handle_swarm_event(identified_listening_at(
        &keypair(1),
        vec![address(OTHER_PUBLIC)],
    ));
    assert_eq!(harness.driver.known_peer_address_count(), 1);
}

#[test]
fn an_override_does_not_stop_the_ledger_recording_what_peers_observe() {
    // P3-7/S2, first half. An assertion is the *weakest* of the three sources
    // of an advertised address, not the strongest: supplying one must not put
    // the peer into a state where it stops listening to what other peers say
    // about it.
    let mut harness = harness_or_skip!();
    assert_eq!(harness.assert_external(PUBLIC), Ok(()));
    assert_eq!(harness.drain(), vec![confirmed_at(PUBLIC)]);

    // Observation of a *different* address still runs the full corroboration
    // path and still promotes.
    harness.identify_then_candidate(&keypair(1), OTHER_PUBLIC);
    assert_eq!(harness.diagnostics.external_candidates_recorded(), 1);
    assert!(confirmed_addresses(&harness.drain()).is_empty());

    harness.identify_then_candidate(&keypair(2), OTHER_PUBLIC);
    assert_eq!(harness.diagnostics.external_addresses_promoted(), 1);
    assert_eq!(harness.drain(), vec![confirmed_at(OTHER_PUBLIC)]);

    // And observation of the asserted address itself is still recorded. The
    // asserted address is deliberately *not* entered into the ledger as
    // already-promoted: doing so would make an operator's claim suppress the
    // evidence about it, which is the thing invariant 3 forbids.
    harness.identify_then_candidate(&keypair(1), PUBLIC);
    assert_eq!(
        harness.diagnostics.external_candidates_recorded(),
        2,
        "evidence about the asserted address is still collected"
    );
}

#[test]
fn an_override_is_still_probed_and_can_still_be_contradicted() {
    // **P3-7/S2, second half, and the point of the whole safeguard.** A user
    // who asserts an address that does not work must still be told it does not
    // work. Piece 2's honesty is not something an operator can switch off by
    // typing a flag, and this is the one place the three pieces could have been
    // made to contradict each other.
    let mut harness = harness_or_skip!();
    assert_eq!(harness.assert_external(PUBLIC), Ok(()));
    assert_eq!(harness.drain(), vec![confirmed_at(PUBLIC)]);

    // Probing continues, and every probe is still counted — the failure mode of
    // this whole area is silence.
    harness.probe_failed(&keypair(1), PUBLIC);
    assert_eq!(harness.diagnostics.probes_run(), 1);
    assert_eq!(harness.diagnostics.probes_failed(), 1);
    assert!(
        verdicts(&harness.drain()).is_empty(),
        "one server's word still is not enough, override or no override"
    );

    harness.probe_failed(&keypair(2), PUBLIC);
    assert_eq!(
        verdicts(&harness.drain()),
        vec![Reachability::Unreachable],
        "an assertion never outranks evidence: two servers agreeing that the \
         asserted address does not answer still reports Unreachable (S2)"
    );

    // And the return trip works from there too, so the assertion has not
    // latched the verdict in either direction.
    harness.probe_succeeded(&keypair(3), PUBLIC);
    assert_eq!(verdicts(&harness.drain()), vec![reachable_at(PUBLIC)]);
    assert_eq!(harness.diagnostics.probes_run(), 3);
    assert_eq!(harness.diagnostics.probes_succeeded(), 1);
}

// --------------------------- the two adapter defects (canvas `0010`, OP-5)

/// A distinct Ed25519 identity per index, for the tests that need more of them
/// than [`keypair`]'s single byte can express.
///
/// The seeds are disjoint from `keypair`'s by construction — that one repeats
/// one byte thirty-two times, this one leaves twenty-eight of them zero — so a
/// flood built here can never collide with a fixture used above.
fn nth_keypair(index: u32) -> Keypair {
    let mut seed = [0_u8; 32];
    seed[..4].copy_from_slice(&index.to_be_bytes());
    Keypair::ed25519_from_bytes(seed).expect("32 bytes are a valid Ed25519 seed")
}

/// One mDNS sighting: a peer, and the LAN address it answers at.
fn lan_sighting(index: u32) -> (Libp2pPeerId, Multiaddr) {
    (
        nth_keypair(index).public().to_peer_id(),
        address(&format!("/ip4/192.168.1.10/tcp/{}", 4001 + index)),
    )
}

#[test]
fn the_same_sightings_are_there_for_a_second_join_as_for_the_first() {
    // **A7, and the regression this operation exists for.** `ObservePeers` was
    // served by `std::mem::take`, so the LAN rung emptied its own input: the
    // first join connected over `local network` and every later one — a
    // rejoin, a reconnect after a drop — reported `local network: nothing to
    // try` with the neighbour still sitting on the link. Observed directly
    // across two joins of one unmoved instance.
    //
    // The assertion is equality across reads, not merely a non-empty second
    // read: a driver that answered twice and forgot on the third would be the
    // same defect one join further along.
    let mut harness = harness_or_skip!();
    harness.mdns_discovered(vec![lan_sighting(1), lan_sighting(2)]);

    let first = harness.observe_peers();
    let second = harness.observe_peers();
    let third = harness.observe_peers();

    assert_eq!(first.len(), 2, "both LAN neighbours are candidates");
    assert_eq!(
        first, second,
        "the second join sees exactly what the first one saw (A7)"
    );
    assert_eq!(second, third, "and so does the third");
}

#[test]
fn a_peer_seen_again_refreshes_its_sighting_instead_of_appearing_twice() {
    // The rung dials candidates one at a time until one answers, so a duplicate
    // entry is a wasted dial and a slower join. mDNS re-announces on a timer
    // and Kademlia routing updates arrive continuously, which makes re-sighting
    // the ordinary case rather than the edge one.
    let mut harness = harness_or_skip!();
    let peer = nth_keypair(1).public().to_peer_id();

    harness.mdns_discovered(vec![(peer, address("/ip4/192.168.1.10/tcp/4001"))]);
    harness.mdns_discovered(vec![(peer, address("/ip4/192.168.1.10/tcp/4001"))]);
    harness.mdns_discovered(vec![(peer, address("/ip4/203.0.113.7/tcp/4001"))]);

    let observed = harness.observe_peers();
    assert_eq!(observed.len(), 1, "one peer, however often it announces");
    assert_eq!(
        observed[0].endpoints.len(),
        2,
        "and both addresses it claimed, each once"
    );
}

#[test]
fn a_flood_of_sightings_cannot_grow_the_buffer_past_its_bound() {
    // **Canvas §7/S6.** A read that no longer empties the buffer is a read that
    // no longer bounds it, so the bound has to be somewhere — and the input is
    // attacker-influenceable in the most direct way there is: mDNS is
    // answerable by any host on the link, and identities are free.
    //
    // Driven at the cap rather than under it, through the real mDNS arm, so
    // what is asserted is the driver's behaviour and not the ledger's unit test
    // repeated.
    let mut harness = match Harness::start_with(ResourceLimits {
        max_observed_peers: 8,
        ..ResourceLimits::DEFAULT
    }) {
        Some(harness) => harness,
        None => return,
    };

    for index in 0..200 {
        harness.mdns_discovered(vec![lan_sighting(index)]);
        // Drained so the bounded event queue never becomes the thing under
        // test: what is bounded here is the sighting buffer.
        let _ = harness.drain();
        assert!(
            harness.observe_peers().len() <= 8,
            "the bound holds at every step, not only at the end"
        );
    }

    assert_eq!(harness.observe_peers().len(), 8);
    assert_eq!(
        harness.diagnostics.dropped_events(),
        0,
        "and the flood was absorbed rather than overflowing anything else"
    );
}

#[test]
fn a_broadcast_that_reached_nobody_is_accepted_and_says_so() {
    // **D11 and the empty half of A6.** A peer alone on the network is
    // `Isolated`, which is a normal status, so this must not become an `Err`
    // that callers treat as a fault — and it must not be indistinguishable from
    // a delivery either, which is what `→ published` was for a run whose
    // broadcasts appeared in no other pane.
    let mut harness = harness_or_skip!();

    assert_eq!(
        harness.publish_broadcast(b"anyone there?"),
        Ok(()),
        "publishing while alone is normal, not an error"
    );

    assert_eq!(
        harness.diagnostics.broadcasts_reaching_nobody(),
        1,
        "and it is visible as what it was"
    );
    assert_eq!(
        harness.diagnostics.broadcasts_propagated(),
        0,
        "nothing was handed to anybody, and nothing claims it was (A6)"
    );
}

#[test]
fn every_broadcast_lands_in_exactly_one_of_the_two_counters() {
    // The counters are the only place the difference survives, so they have to
    // account for every publish: a call that incremented neither would restore
    // the silence the whole change is about, one release later and invisibly.
    let mut harness = harness_or_skip!();

    for attempt in 0..5 {
        assert_eq!(
            harness.publish_broadcast(format!("{attempt}").as_bytes()),
            Ok(())
        );
    }

    assert_eq!(
        harness.diagnostics.broadcasts_propagated()
            + harness.diagnostics.broadcasts_reaching_nobody(),
        5
    );
}

#[test]
fn an_oversize_broadcast_is_refused_and_counted_as_neither_outcome() {
    // The refusal path is a *failure*, not a state: nothing was published, so
    // neither publishing counter may move. Asserted because the easy way to
    // write the arm above is to count first and decide afterwards.
    let mut harness = harness_or_skip!();
    let oversize = vec![0_u8; harness.limits.max_envelope_bytes + 1];

    assert_eq!(
        harness.publish_broadcast(&oversize),
        Err(MessageTransportError::Unavailable)
    );

    assert_eq!(harness.diagnostics.oversize_frames(), 1);
    assert_eq!(harness.diagnostics.broadcasts_propagated(), 0);
    assert_eq!(harness.diagnostics.broadcasts_reaching_nobody(), 0);
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
