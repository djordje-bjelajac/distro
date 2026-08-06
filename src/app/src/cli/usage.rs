use crate::cli::ProfileDirectory;

/// The text `--help` prints.
///
/// # Two sentences that are not optional
///
/// Safeguards S7 and S8 require that two facts be *told to the user*, not left
/// to be discovered:
///
/// * **S7 — the connectivity limit.** Two peers both behind symmetric NAT, with
///   no publicly reachable peer online to relay for them, cannot connect. That
///   is inherent to a network with no operator-run infrastructure, not a defect
///   and not something a retry fixes.
/// * **S8 — the privacy disclosure.** Joining announces this peer's addresses
///   to the network, and the broadcast channel is public to every member.
///
/// They are in the help text and in `README.md`, and the same two sentences are
/// reachable inside the UI with `?`. A safeguard a user has to read the source
/// to learn is not a disclosure.
pub struct Usage;

impl Usage {
    /// The program name as a user types it.
    pub const PROGRAM: &'static str = "distro";

    /// Renders the full usage text.
    pub fn text() -> String {
        format!(
            "\
{program} {version} — serverless peer-to-peer text messaging

USAGE
    {program} [OPTIONS]

    With no options at all this launches, creates an identity if none exists,
    joins whatever network it can reach, and opens the terminal UI. There is no
    registration step and nothing to configure.

OPTIONS
    --profile <DIR>     Where this instance keeps its identity, trust records,
                        peer cache and sequence counter. Overrides
                        ${environment_variable}. Give two instances two
                        directories to run both on one machine.
    --ticket <STRING>   A join ticket to fall back on when the peer cache and
                        the local network turn up nothing. Needed once, for a
                        fresh install's first internet join.
    --topic <NAME>      The broadcast topic. This is a network identifier as
                        much as a topic name: peers on different topics are on
                        different networks. Default: {default_topic}
    --listen <ADDR>     A multiaddress to bind, repeatable. Not a bootstrap
                        list: these are addresses this peer listens on, not
                        hosts it contacts. The default binds every interface on
                        a port the OS picks, which is right for two instances
                        on one machine and wrong for a peer that wants to be
                        found again after a restart — pin a port for that, and
                        for a port forwarded through a NAT.
                        e.g. --listen /ip4/0.0.0.0/udp/4001/quic-v1
    --external-address <ADDR>
                        A multiaddress the world reaches this peer at,
                        repeatable. Not a bootstrap list either. The value is
                        this peer's own address, not a host to contact: it is
                        advertised so other peers can reach this one, and it is
                        never dialled. Use it when you have forwarded a port
                        and no other peer has observed the address for you yet.
                        Saying an address works does not make it work: it is
                        still probed, and one that does not answer is reported
                        unreachable rather than believed.
                        e.g. --external-address /ip4/203.0.113.7/tcp/4001
    --no-lan            Do not discover peers over mDNS on the local link.
    --print-identity    Print this profile's identity and exit. Starts no
                        network and opens no terminal.
    -h, --help          Print this text.
    -V, --version       Print the version.

PROFILE DIRECTORY
    1. --profile <DIR>
    2. ${environment_variable}
    3. $XDG_DATA_HOME/distro, or $HOME/.local/share/distro

JOINING
    Three paths are tried in order and the first that answers wins: peers
    cached from previous sessions, peers on the local network, then a join
    ticket. A ticket is a string any member can generate (press `g` in the UI)
    and hand over out of band. After a first successful join, tickets are never
    needed again on that machine.

    Reaching no peer at all is a normal state, not a failure: the status line
    says `isolated` and the reason each path produced nothing is shown.

WHAT YOU SHOULD KNOW BEFORE JOINING
    * Joining announces this peer's network addresses to the network. Messages
      on the broadcast channel are readable by every member — that is what a
      network-wide channel is. Direct messages are encrypted end to end by the
      transport, including when a third peer relays them. Nothing else leaves
      this machine, and there is no telemetry.

    * Two peers that are both behind symmetric NAT cannot connect unless some
      peer that is publicly reachable is online to relay for them. There is no
      infrastructure to fall back on, because there is no infrastructure: every
      relay on this network is another user's instance. When it happens the UI
      says so — a direct message fails with `no relay available` rather than
      retrying forever.

    * Conversation history is held in memory and is gone when this process
      exits. The identity, the trust records, the peer cache and the outbound
      sequence counter persist in the profile directory.
",
            program = Self::PROGRAM,
            version = env!("CARGO_PKG_VERSION"),
            environment_variable = ProfileDirectory::ENVIRONMENT_VARIABLE,
            default_topic = infra_net_libp2p::NetworkConfig::DEFAULT_TOPIC,
        )
    }

    /// The version line `--version` prints.
    pub fn version() -> String {
        format!("{} {}", Self::PROGRAM, env!("CARGO_PKG_VERSION"))
    }

    /// The disclosures S7 and S8 require, condensed for the UI's help
    /// overlay — the same facts as the help text, in the space a pane has.
    pub const DISCLOSURES: [&'static str; 4] = [
        "Joining announces your addresses to the network.",
        "Broadcast messages are readable by every member; directs are encrypted end to end.",
        "Two symmetric-NAT peers cannot connect unless a publicly reachable peer is online to relay.",
        // S8 as amended 2026-08-07 (canvas `0013`). The clipboard is the one
        // place this build puts a peer's addresses somewhere it does not
        // control, and on a host running a syncing clipboard manager they
        // leave the machine entirely — which is what the safeguard used to
        // say could not happen.
        "Copying a join ticket puts your addresses on the system clipboard; a syncing clipboard manager will carry them to your other devices.",
    ];
}
