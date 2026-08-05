//! The one thing in the binary worth asserting: that an option a user typed
//! reaches the configuration the network is built from.
//!
//! Everything else here is exit codes and the terminal's raw mode. This is not,
//! because a field that silently fails to be copied across looks — from the
//! outside — exactly like an option that does nothing, which is the failure
//! `--external-address` is least able to afford.

use app::cli::LaunchOptions;
use infra_net_libp2p::NetworkConfig;

use crate::network_of;

#[test]
fn a_launch_that_asserts_nothing_configures_no_external_address() {
    let network = network_of(LaunchOptions::default());

    assert!(
        network.external_addresses.is_empty(),
        "the ordinary launch asserts nothing, and there is no address this \
         program could guess that would not be a lie"
    );
}

#[test]
fn an_external_address_option_reaches_the_network_configuration() {
    let network = network_of(LaunchOptions {
        external_addresses: vec!["/ip4/203.0.113.7/tcp/4001".to_owned()],
        ..LaunchOptions::default()
    });

    assert_eq!(
        network.external_addresses,
        vec!["/ip4/203.0.113.7/tcp/4001".to_owned()]
    );
}

#[test]
fn repeated_external_addresses_reach_the_configuration_in_the_order_supplied() {
    let network = network_of(LaunchOptions {
        external_addresses: vec![
            "/ip4/203.0.113.7/tcp/4001".to_owned(),
            "/ip4/203.0.113.7/udp/4001/quic-v1".to_owned(),
        ],
        ..LaunchOptions::default()
    });

    assert_eq!(
        network.external_addresses,
        vec![
            "/ip4/203.0.113.7/tcp/4001".to_owned(),
            "/ip4/203.0.113.7/udp/4001/quic-v1".to_owned(),
        ],
        "the order is the operator's and is preserved: it is the order the \
         addresses are advertised in"
    );
}

#[test]
fn an_asserted_address_never_becomes_something_to_listen_on_or_dial() {
    // S1, at the one seam where the two lists could be crossed. An external
    // address is where this peer says it can be found; a listen address is
    // where it binds. Neither is a host to contact, and neither is the other.
    let network = network_of(LaunchOptions {
        external_addresses: vec!["/ip4/203.0.113.7/tcp/4001".to_owned()],
        ..LaunchOptions::default()
    });

    assert_eq!(
        network.listen_addresses,
        NetworkConfig::default().listen_addresses,
        "asserting an address must not change what this peer binds"
    );
}

#[test]
fn the_other_options_still_land_where_they_did() {
    let network = network_of(LaunchOptions {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/45011".to_owned()],
        broadcast_topic: Some("/distro/test/1.0.0".to_owned()),
        lan_discovery: false,
        ..LaunchOptions::default()
    });

    assert_eq!(
        network.listen_addresses,
        vec!["/ip4/127.0.0.1/tcp/45011".to_owned()]
    );
    assert_eq!(network.broadcast_topic, "/distro/test/1.0.0");
    assert!(!network.enable_lan_discovery);
}
