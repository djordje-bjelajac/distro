//! What the driver does with an external-address candidate, and with an
//! AutoNAT probe report.
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

use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};

use libp2p::gossipsub::IdentTopic;
use libp2p::identity::Keypair;
use libp2p::swarm::{ConnectionId, SwarmEvent};
use libp2p::{Multiaddr, PeerId as Libp2pPeerId, autonat, identify};
// Two different questions share this name: the domain's is a property of one
// *address* (direct or relayed), this crate's is a property of *this peer's*
// position on the network. Aliased rather than qualified so neither reads as
// the other.
use membership::domain::Reachability as EndpointReachability;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::codec::{CodecDiagnostics, EnvelopeCodec};
use crate::mapping::EndpointMapping;
use crate::runtime::network_runtime::build_swarm;
use crate::runtime::{NetworkConfig, NetworkIdentity};
use crate::swarm::NetworkEvent;
use crate::swarm::distro_behaviour::DistroBehaviourEvent;
use crate::swarm::network_command::NetworkCommand;
use crate::swarm::network_driver::NetworkDriver;
use crate::swarm::reachability_ledger::{ProbeResult, Reachability};
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
