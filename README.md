# distro

Serverless peer-to-peer text messaging. One binary, no account, no
configuration, and **no infrastructure** — no bootstrap host, no relay server,
no rendezvous point, no telemetry endpoint. The discovery, relaying and
hole-punch coordination a network like this needs are provided by the
participants: every instance offers all three to every other instance, and there
is no flag that turns any of them off. A peer that relays for you is somebody
else running the same program.

There are two places to talk: one network-wide broadcast channel, and 1:1
direct conversations with any peer you can reach.

## Build and run

```bash
cargo build --workspace
./target/debug/distro                 # launch: creates an identity, joins, opens the UI
./target/debug/distro --help          # every option, and the disclosures below
```

First launch creates an Ed25519 keypair in a profile directory
(`$XDG_DATA_HOME/distro`, or `$HOME/.local/share/distro`) and derives your
identity from it. Nothing asks you anything; there is no registration step,
because there is nothing to register with.

The interface is a terminal UI. `?` shows the keys and repeats the disclosures.

## How two peers find each other

Three paths, tried in that order, and the first that answers wins:

1. **Cached peers.** Peers from previous sessions, saved when you quit.
2. **The local network.** mDNS on the local link, unconfigured. Two instances
   on one LAN find each other with no ticket and no setup. Switchable off with
   `--no-lan`.
3. **A join ticket.** A string any member can generate (`g` in the UI) and hand
   over out of band — chat, email, a photo of a screen. The recipient pastes it
   (`p`, or `--ticket <STRING>` at startup).

The honest cost of having no servers: **the first-ever internet join of a fresh
install needs one pasted ticket.** There is no way around it that does not
introduce a host somebody operates. After that first join the peer cache carries
you, and on a LAN a ticket is never needed at all.

Reaching nobody is a normal state, not a failure. The status line says
`isolated`, and the notices pane says what each of the three paths produced.

## What you should know before joining

Two facts this project states rather than leaves to be discovered. They are also
in `--help` and behind `?` in the interface.

**Joining is public.** Joining announces this peer's network addresses to the
network. Messages on the broadcast channel are readable by every member — that
is what a network-wide channel is. Direct messages are encrypted end to end by
the transport, including when a third peer relays them; a relay carries
ciphertext it cannot read. Nothing else leaves the machine, and there is no
telemetry.

**Some pairs of peers cannot connect.** If two peers are both behind symmetric
NAT and no publicly reachable peer is online to relay for them, they cannot
reach each other. This is inherent to a network with no operator-run
infrastructure: every relay here is another user's instance. It is not a defect
and no amount of retrying fixes it — a direct message fails with
`no relay available` and says so.

## What v1 is not

Text only. Conversation history lives in memory and is gone when the process
exits — only the identity, the trust records, the peer cache and the outbound
sequence counter persist. A peer that is offline when you write to it does not
receive the message later; there is no store-and-forward. A peer joining the
broadcast channel sees what is said from then on, not what was said before. No
voice, no video, no file transfer, no rooms, no editing or deleting a message,
no multi-device identity, no mobile build.

Trust is trust-on-first-use: compare fingerprints out of band (`f`) before
marking a peer verified (`v`). Blocking (`b`) is purely local — it stops content
reaching you and tells no one.

## Where things are

| | |
| --- | --- |
| [`src/app/README.md`](src/app/README.md) | running it: options, keys, the profile directory, and the two-instance procedure |
| [`docs/specs/`](docs/specs/) | the REASONS canvas this is built from, and the analysis behind it |
| [`AGENTS.md`](AGENTS.md) | contributing: layout, dependency rules, the four checks |

The canvas is the implementation source of truth. Its safeguards — no
operator-run infrastructure, wire compatibility from the first commit,
hostile-input caps, determinism in tests — are checked by the test suite, not
only asserted in prose.

## Checks

```bash
cargo test --workspace
cargo build --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

A handful of tests need to bind a local socket and skip, loudly, where the
machine forbids it. Set `DISTRO_REQUIRE_NETWORK_TESTS=1` to turn that skip into
a failure — on any machine that has networking, that is the setting you want.
