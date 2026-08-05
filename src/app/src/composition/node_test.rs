use identity::ports::{IdentityCommandPort, IdentityKeyStorePort, IdentityQueryPort};
use infra_net_libp2p::{NetworkConfig, NetworkStartError};
use infra_store_fs::LocalStores;
use membership::ports::MembershipQueryPort;
use messaging::domain::{ConversationId, MessageBody};
use messaging::ports::{AuthorPolicyPort, MessagingQueryPort, SendMessagePort};
use shared_types::{Fingerprint, ProtocolVersion};

use crate::composition::{Node, NodeSettings, StartError};
use crate::test_dir::TestDir;
use crate::test_peers::carol;

/// Starts a node on a throwaway profile, or reports why it could not.
///
/// # Why this test exists at all
///
/// [`Node::start`] is a hundred and thirty lines that nothing else executes:
/// the startup order, the S3a secret handoff and its zeroize paths, the four
/// crypto ports resolved to one object, `TrustDirectory` standing in as
/// `messaging`'s `AuthorPolicyPort`, and the `CorrelatingTransport` wrap. Every
/// one of those was proven by reading. A wiring mistake in any of them — the
/// wrong `Arc` handed to the wrong port, a coercion that compiles and means
/// something else — would first be noticed by a user.
///
/// The canvas asked (OP-12) for a wiring smoke test "against sim-net". It
/// cannot be that: `app` must never link `infra-sim-net` (canvas OP-8, and
/// `Cargo.toml` says so). This is the same test in the only form the dependency
/// rules allow — the real composition root over the real adapters, on loopback,
/// listening to nothing.
fn start(label: &str) -> Option<(TestDir, Node)> {
    let directory = TestDir::new(label);
    let settings = NodeSettings {
        profile_directory: directory.path().to_path_buf(),
        // Loopback and no mDNS: nothing leaves the machine and no multicast
        // packet reaches a developer's LAN (AC13).
        network: NetworkConfig::loopback(),
    };

    match Node::start(&settings) {
        Ok(node) => Some((directory, node)),
        Err(error @ StartError::Network(_)) => {
            // A machine that cannot start a swarm is a fact about the machine.
            // `DISTRO_REQUIRE_NETWORK_TESTS=1` says it is not.
            crate::required_network::skip(&error);
            None
        }
        Err(error) => {
            panic!("the node failed to start for a reason that is not the network: {error}")
        }
    }
}

#[test]
fn a_node_starts_on_an_empty_profile_and_is_the_peer_its_key_file_says_it_is() {
    // AC1 and AC9 through the composition root rather than through a store: no
    // configuration, no registration, and the identity the swarm authenticates
    // as is the identity on disk.
    let Some((directory, node)) = start("start") else {
        return;
    };

    let stores = LocalStores::open(directory.path()).expect("the profile directory opens");
    let on_disk = stores
        .identity_keys()
        .load_or_create_local_peer()
        .expect("the key file the node just wrote is readable");

    assert_eq!(
        node.local_peer(),
        on_disk,
        "the swarm must authenticate as the peer the key store holds — this is \
         the S3a secret handoff, and a mismatch means the transport identity and \
         the signing identity came from different keys"
    );

    // Step 2 of the startup order: the identity was *assumed*, not merely
    // loaded. Nothing asked the user anything (AC1).
    let summary = node
        .identity()
        .queries()
        .local_identity()
        .expect("the identity is initialized before start returns");
    assert_eq!(summary.peer, node.local_peer());
    assert_eq!(
        summary.fingerprint,
        Fingerprint::of(&node.local_peer()),
        "the display name is derived from this peer's own fingerprint"
    );

    assert_eq!(node.profile_directory(), directory.path());
    assert_eq!(node.protocol(), ProtocolVersion::CURRENT);

    node.shutdown();
}

#[test]
fn the_same_profile_comes_back_as_the_same_peer() {
    // AC9's actual claim — stable across restarts — exercised through the whole
    // root, which means the *load* path of the keystore and of the transport
    // secret, not just the create path the first launch takes.
    let directory = TestDir::new("restart");
    let settings = NodeSettings {
        profile_directory: directory.path().to_path_buf(),
        network: NetworkConfig::loopback(),
    };

    let first = match Node::start(&settings) {
        Ok(node) => node,
        Err(error @ StartError::Network(_)) => return crate::required_network::skip(&error),
        Err(error) => panic!("the node failed to start: {error}"),
    };
    let peer = first.local_peer();
    first.shutdown();

    let second = Node::start(&settings).expect("the same profile starts again");
    assert_eq!(
        second.local_peer(),
        peer,
        "a restart must not mint a new identity (AC9)"
    );

    second.shutdown();
}

#[test]
fn the_block_list_identity_owns_is_the_one_messaging_asks() {
    // Invariant 11's wiring: `messaging` declares `AuthorPolicyPort`, `identity`
    // holds the answer, and the root joins them with `TrustDirectory` — which
    // is a coercion that compiles just as happily if it is given the wrong
    // store. Blocking through one context and asking through the other is the
    // only thing that tells the two cases apart.
    let Some((_directory, node)) = start("block") else {
        return;
    };

    let policy: &dyn AuthorPolicyPort = node.trust().as_ref();
    assert!(
        !policy.is_blocked(carol()),
        "nobody is blocked on a fresh profile"
    );

    node.identity()
        .commands()
        .block_peer(carol())
        .expect("blocking is a local decision and cannot fail here");
    node.trust()
        .refresh(&[carol()])
        .expect("the root refreshes its cache of the block list");

    assert!(
        policy.is_blocked(carol()),
        "a peer blocked through `identity` must be blocked for `messaging`, or \
         invariant 11 has no enforcement site"
    );

    node.shutdown();
}

#[test]
fn a_broadcast_sent_through_the_root_is_signed_by_this_peer_and_recorded() {
    // The one path that touches nearly everything the root wired: the sequence
    // counter, the signer behind `messaging`'s own `EnvelopeSignerPort`, the
    // `AuthorPolicyPort`, the `CorrelatingTransport` wrapping the real
    // libp2p transport, the message log, and the event sink. A broadcast with
    // nobody subscribed is success (D3), so this asserts the composition, not
    // the network.
    let Some((_directory, node)) = start("broadcast") else {
        return;
    };

    let body = MessageBody::new("the wiring holds").expect("a valid body");
    let outcome = node
        .messaging()
        .send()
        .publish_broadcast(body)
        .expect("publishing to a topic nobody is listening to is success");

    assert_eq!(
        outcome.sent.id.author(),
        node.local_peer(),
        "the author is the peer whose signature verifies (invariant 4), so a \
         mismatch means `messaging` was handed a signer over a different key"
    );

    let history = node
        .messaging()
        .queries()
        .history(ConversationId::Broadcast);
    assert_eq!(history.len(), 1, "the message reached the read model");
    assert_eq!(history[0].author(), node.local_peer());

    node.shutdown();
}

#[test]
fn a_node_that_has_not_joined_is_isolated_and_has_no_ticket_to_give() {
    // Startup step 4: the swarm is running and nothing is listening. Joining is
    // a separate decision and the root does not take it (AC3 — `Isolated` is a
    // normal state, not an error), so there is no endpoint yet and therefore no
    // ticket to mint.
    let Some((_directory, node)) = start("isolated") else {
        return;
    };

    assert_eq!(
        node.membership().queries().network_status(),
        membership::domain::NetworkStatus::Isolated
    );
    assert!(node.endpoints().all().is_empty());
    assert_eq!(
        node.mint_join_ticket(),
        Err(membership::domain::JoinTicketError::NoEndpoints),
        "a ticket with nothing to dial is not a bootstrap credential"
    );

    // Nothing has been rejected, tolerated, or dropped: no peer has spoken to
    // this instance at all (S2, S6).
    let diagnostics = node.codec_diagnostics();
    assert_eq!(diagnostics.rejected_major(), 0);
    assert_eq!(diagnostics.malformed_frames(), 0);
    assert_eq!(diagnostics.dropped_events(), 0);

    // And nothing was asserted: a launch with no `--external-address` must not
    // read like one that had it and lost it (D6).
    assert!(
        node.diagnostics().external_addresses_supplied().is_empty(),
        "the ordinary launch asserts no address"
    );
    assert!(node.diagnostics().external_addresses_in_effect().is_empty());

    node.shutdown();
}

/// The address the asserted-override tests use.
///
/// RFC 5737 documentation space: globally routable as far as the predicate is
/// concerned, and nobody's real host — so a test that advertises it cannot send
/// a stranger's peer anywhere.
const ASSERTED: &str = "/ip4/203.0.113.7/tcp/4001";

#[test]
fn an_asserted_external_address_reaches_the_network_and_is_reported_as_supplied() {
    // The wiring OP-3 adds, end to end: a value in `NetworkConfig` survives
    // `NetworkRuntime::start` — which refuses malformed and non-global ones
    // before building anything — and is reported by the diagnostics the `d`
    // overlay reads.
    let directory = TestDir::new("external-supplied");
    let settings = NodeSettings {
        profile_directory: directory.path().to_path_buf(),
        network: NetworkConfig {
            external_addresses: vec![ASSERTED.to_owned()],
            ..NetworkConfig::loopback()
        },
    };

    let node = match Node::start(&settings) {
        Ok(node) => node,
        Err(error @ StartError::Network(_)) => return crate::required_network::skip(&error),
        Err(error) => panic!("the node failed to start: {error}"),
    };

    assert_eq!(
        node.diagnostics().external_addresses_supplied(),
        vec![ASSERTED.to_owned()],
        "what the operator asked for is a launch-time fact and is known the \
         moment the node exists"
    );
    // The other half of D6, and the reason these are two lists: the
    // confirmation the swarm queued is drained by the engine, which is not
    // running here. Supplied is not in effect, and nothing pretends otherwise.
    assert!(
        node.diagnostics().external_addresses_in_effect().is_empty(),
        "an assertion reports itself as in effect only once the network says \
         so — the root cannot infer one from the other (D6, S4)"
    );

    node.shutdown();
}

#[test]
fn a_private_external_address_refuses_the_launch_and_says_mdns_already_covers_it() {
    // P3-8 through the composition root: the globality predicate belongs to the
    // adapter, and this is the assertion that it is actually reached from here
    // — a launch that started anyway would advertise a LAN address to the
    // internet and look like it had worked.
    let directory = TestDir::new("external-private");
    let settings = NodeSettings {
        profile_directory: directory.path().to_path_buf(),
        network: NetworkConfig {
            external_addresses: vec!["/ip4/192.168.1.10/tcp/4001".to_owned()],
            ..NetworkConfig::loopback()
        },
    };

    let error = match Node::start(&settings) {
        Err(error) => error,
        Ok(node) => {
            node.shutdown();
            panic!("a private external address must refuse the launch (P3-8)")
        }
    };

    match error {
        StartError::Network(NetworkStartError::NonGlobalExternalAddress) => assert!(
            error.to_string().contains("mDNS"),
            "somebody who typed their LAN address needs to be told the local \
             link is already covered, not merely that they were refused: \
             {error}"
        ),
        // The predicate runs after the swarm is built, so a machine that cannot
        // build one fails earlier and for a different reason. That is a fact
        // about the machine, and `DISTRO_REQUIRE_NETWORK_TESTS=1` says it is
        // not.
        StartError::Network(_) => crate::required_network::skip(&error),
        other => panic!("the launch failed for a reason that is not the address: {other}"),
    }
}
