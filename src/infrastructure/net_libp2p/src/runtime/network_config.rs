use std::time::Duration;

use shared_types::ProtocolVersion;

use crate::limits::ResourceLimits;

/// Everything a peer needs to decide about its own network, in one value.
///
/// # There is no bootstrap list, and there never will be
///
/// The field a P2P library normally has here — "bootstrap peers", "relay
/// servers", "rendezvous point" — is absent by design (S1). Every peer this
/// instance ever learns about comes from its own cache, its own LAN, a ticket a
/// human handed it, or another peer it already knew. There is no default value
/// this crate could ship that would not be a host somebody else operates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    /// Multiaddresses to listen on.
    ///
    /// Port `0` by default, so the OS picks a free one: a fixed default port
    /// would collide with a second instance on the same machine, and any port
    /// this peer picks is published by `announce` anyway.
    pub listen_addresses: Vec<String>,
    /// D1 rung (b): unconfigured LAN discovery over mDNS.
    ///
    /// The one thing in this build that speaks without being asked, and only
    /// to the local link. Off in tests, so a test run never multicasts onto a
    /// developer's real network.
    pub enable_lan_discovery: bool,
    /// D3's single broadcast topic. Every member of a network must use the
    /// same string, so this is a network identifier as much as a topic name.
    pub broadcast_topic: String,
    /// The protocol version this build speaks (S2). Every inbound envelope is
    /// judged against it.
    pub protocol_version: ProtocolVersion,
    /// How long a connection with no open stream is kept.
    ///
    /// libp2p's default is zero — a connection with nothing on it closes
    /// immediately — which would make every session flap between messages. A
    /// minute keeps a conversation's link alive across ordinary human pauses
    /// while still letting an abandoned peer's connection go.
    pub idle_connection_timeout: Duration,
    /// The S6 caps.
    pub limits: ResourceLimits,
}

impl NetworkConfig {
    /// The default broadcast topic (D3).
    pub const DEFAULT_TOPIC: &'static str = "/distro/broadcast/1.0.0";

    /// A configuration that binds only to loopback and does not multicast.
    ///
    /// For tests: the OS picks the ports, nothing leaves the machine, and no
    /// mDNS packet reaches the developer's LAN (AC13's "no external network").
    pub fn loopback() -> Self {
        Self {
            listen_addresses: vec![
                "/ip4/127.0.0.1/udp/0/quic-v1".to_owned(),
                "/ip4/127.0.0.1/tcp/0".to_owned(),
            ],
            enable_lan_discovery: false,
            ..Self::default()
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addresses: vec![
                "/ip4/0.0.0.0/udp/0/quic-v1".to_owned(),
                "/ip4/0.0.0.0/tcp/0".to_owned(),
            ],
            enable_lan_discovery: true,
            broadcast_topic: Self::DEFAULT_TOPIC.to_owned(),
            protocol_version: ProtocolVersion::CURRENT,
            idle_connection_timeout: Duration::from_secs(60),
            limits: ResourceLimits::DEFAULT,
        }
    }
}
