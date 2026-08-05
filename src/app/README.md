# `distro` — running it

The composition root and the terminal interface (canvas D8, D9, OP-12). Build
with `cargo build --workspace`; the binary is `target/debug/distro`.

```
distro                                # launch with no configuration at all
distro --help                         # every option, and the two disclosures below
distro --print-identity               # report this profile's identity and exit
distro --profile ~/.distro-b          # a second instance on the same machine
distro --ticket distro-join-1.…       # first internet join of a fresh install
distro --listen /ip4/0.0.0.0/udp/4001/quic-v1   # bind a known port
distro --external-address /ip4/203.0.113.7/udp/4001/quic-v1   # announce a forwarded port
```

## What you should know before joining

Two facts the canvas requires be stated rather than discovered (safeguards S7
and S8). They are also in `--help` and behind `?` in the interface.

**Joining is public.** Joining announces this peer's network addresses to the
network. Messages on the broadcast channel are readable by every member — that
is what a network-wide channel is. Direct messages are encrypted end to end by
the transport, including when a third peer relays them; a relay carries
ciphertext it cannot read. Nothing else leaves the machine and there is no
telemetry.

**Some pairs of peers cannot connect.** If two peers are both behind symmetric
NAT and no publicly reachable peer is online to relay for them, they cannot
reach each other. This is inherent to a network with no operator-run
infrastructure: every relay here is another user's instance. It is not a defect
and no amount of retrying fixes it — a direct message fails with
`no relay available` and says so.

**History is not kept.** Conversations live in memory and are gone when the
process exits. The identity, the trust records, the peer cache and the outbound
sequence counter persist in the profile directory.

## Being reachable from outside

An instance normally learns its own public address rather than being told it:
either several peers independently report seeing the same one, or an AutoNAT
probe dials it back successfully. Both need a peer that is already there. A home
server that is the *first* instance on its network has neither, so a forwarded
port sits working while the peer waits for somebody to notice it.

`--external-address <MULTIADDR>` breaks that deadlock. It is repeatable, and
each value is advertised from startup — in announcements, in DHT records, and in
any join ticket minted afterwards:

```bash
./target/debug/distro \
    --listen /ip4/0.0.0.0/udp/4001/quic-v1 \
    --listen /ip4/0.0.0.0/tcp/4001 \
    --external-address /ip4/203.0.113.7/udp/4001/quic-v1 \
    --external-address /ip4/203.0.113.7/tcp/4001
```

Pin the ports with `--listen` as well, or the OS picks one and the address you
asserted points at nothing.

**It is this peer's own address, and it is never dialled.** The option is the
same shape as the bootstrap list this project does not have — a multiaddress on
the command line — and unrelated to it. Nothing passes the value to a dial, to
the peer cache, or to the DHT as somebody else's address; it is advertised so
that other peers can reach *this* one, and that is all it does.

**Asserting an address does not make it work.** It is the weakest of the three
sources, not the strongest: observation and probing carry on exactly as before,
and if a probe finds the asserted address does not answer, the status line says
`unreachable`. You are told your claim was wrong rather than reassured it was
right.

**A malformed value refuses the launch**, naming the value, and so does a
non-global one — `192.168.x.x`, `10.x.x.x`, a loopback or link-local address,
or anything behind `/p2p-circuit`. The refusal for a private address says that
mDNS already covers the local network, because someone typing their LAN address
is usually reaching for an option they do not need.

The `d` overlay reports the two facts separately:

| row | what it means |
| --- | --- |
| `external addresses supplied` | how many `--external-address` values this launch was given |
| `external addresses in effect` | how many of them the network confirmed and is advertising |

`1` and `0` is the interesting case: the flag was set and did not take. They are
two rows because they come from two places — the first from the command line,
the second from the network — and neither can be inferred from the other.

## The profile directory

Everything that outlives the process, in one directory: the Ed25519 keypair
(`identity.key`, `0600`), the trust records, the peer cache, and the outbound
sequence counter. Resolved in this order:

1. `--profile <DIR>`
2. `DISTRO_PROFILE_DIR`
3. `$XDG_DATA_HOME/distro`, or `$HOME/.local/share/distro`

**Two instances need two directories.** Sharing one means sharing a `PeerId`,
which is not two peers, and racing on the sequence counter, which is worse
because it is silent.

## Environment

| variable | what it does |
| --- | --- |
| `DISTRO_PROFILE_DIR` | the profile directory, when `--profile` is not given |
| `DISTRO_REQUIRE_NETWORK_TESTS` | **tests only.** Turns a skipped network test into a failing one |

A few tests need to bind a local socket: the composition-root smoke tests in
this crate and the two-swarm loopback tests in `infra-net-libp2p`. Where a
sandbox forbids that, they skip — and a skip is announced on stderr rather than
swallowed by the test harness, because a test that quietly proves nothing is
worse than one that fails.

Set `DISTRO_REQUIRE_NETWORK_TESTS=1` and the skip becomes a failure instead:

```bash
DISTRO_REQUIRE_NETWORK_TESTS=1 cargo test --workspace
```

That is the setting for CI and for any machine that is supposed to have
networking. Without it, a machine that lost the ability to bind a socket would
report a green suite it never ran. Any value but `0` or the empty string counts
as set.

## Keys

| key | what it does |
| --- | --- |
| `Tab` / `↓` / `j` | next conversation |
| `Shift+Tab` / `↑` / `k` | previous conversation |
| `i` or `Enter` | write a message |
| `Enter` | send |
| `Esc` | cancel, or close an overlay |
| `c` | connect to the selected peer |
| `f` | show full fingerprints — compare these out of band before verifying |
| `v` | verify the selected peer |
| `b` | block or unblock the selected peer |
| `g` | generate a join ticket to hand out |
| `p` | paste a join ticket and join with it |
| `r` / `l` | rejoin / leave the network |
| `d` | local diagnostic counters, whether an asserted address took effect, and which profile this instance is on |
| `?` | help |
| `q` or `Ctrl+C` | quit |

## Running two instances (the OP-13 protocol, on one machine)

```bash
cargo build --workspace

# Terminal 1
./target/debug/distro --profile /tmp/distro-a --no-lan

# In it: press `g`, copy the ticket (it wraps across lines — copy all of it).

# Terminal 2
./target/debug/distro --profile /tmp/distro-b --no-lan --ticket 'distro-join-1.…'
```

Instance A's status line goes `joining` → `isolated` → `connected (1 peer)` as B
redeems the ticket, and a broadcast typed in either appears in the other.
`--no-lan` keeps mDNS off, so this exercises the ticket rung specifically; drop
it to test LAN discovery (AC2), which will multicast on the local link.

`--print-identity` on each profile tells you which `PeerId` is which before you
start, and the `d` overlay tells you which profile a running instance is on.

Quitting with `q` leaves the network properly: sessions close, the departure is
announced, and **the peer cache is saved**. That cache is the first bootstrap
rung, so relaunching both with no ticket at all reconnects them.

### A caveat about ports and the warm start

By default a peer binds port `0` and the OS picks one — right for two instances
on one machine, and wrong for being found again after a restart, because the
peer comes back at a *different* address and every cached entry for it is stale.
For a warm start to work across restarts, pin the ports:

```bash
./target/debug/distro --profile /tmp/distro-a --no-lan \
    --listen /ip4/127.0.0.1/udp/45011/quic-v1 --listen /ip4/127.0.0.1/tcp/45011
./target/debug/distro --profile /tmp/distro-b --no-lan \
    --listen /ip4/127.0.0.1/udp/45012/quic-v1 --listen /ip4/127.0.0.1/tcp/45012
```

Both then reconnect from the cache with no ticket. Two peers starting at the
same instant each wait out their own dial first, so allow ~20 s before
concluding the warm start failed — the notices pane shows the account either
way. On a real network this matters less: the cache holds many peers and some
are still where they were.

## Reading a conversation

Messages are grouped **by author**, not interleaved. `MessagingQueryPort::history`
orders one author's messages in that author's send order and provides no order
across authors — with no global clock and no consensus there is nothing to
derive one from. Sorting by the sender's claimed timestamp would invent a
chronology out of two unsynchronised clocks, one of which the remote peer
chooses freely. The pane shows what the domain can back.

A run of messages this peer gave up waiting for is marked in place, in the
author's block, as `N messages from … were never received`. Silence is not a
state in either direction: outbound messages carry `pending`, `delivered` or
`failed: reason`.
