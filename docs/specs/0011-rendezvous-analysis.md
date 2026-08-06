# SPDD Analysis — Rendezvous Instances

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement:** optional, community-run instances that act as rendezvous points where peers find each other automatically instead of by pasted ticket. A registration must be validated cryptographically so an identity cannot be claimed by a different key.
**Constraint carried in:** this reverses canvas `0002` D1 and safeguard S1. S1's *purpose* — nobody can switch the network off — is to be preserved, not discarded.

---

## 1. Repository evidence

### E1 — The ladder genuinely has no rung left, and the code says so

`JoinNetworkHandler::handle` walks `BootstrapRung::LADDER` — cached peers, LAN mDNS, pasted ticket — and stops at the first peer that answers (`membership/src/application/commands/join_network.rs:129-138`). Each rung's failure mode is exactly the one the user hit:

- `walk_cached_peers` dials `PeerCachePort::load()` — addresses recorded when the peer was last seen. Nothing refreshes them while this machine is off.
- `walk_local_network` reaches only the broadcast domain.
- `walk_join_ticket` needs a `JoinTicket`, and minting one requires a live peer with current addresses.

`BootstrapRung`'s own doc (`ports/bootstrap_rung.rs:17-20`) states "There is deliberately no fourth rung", naming "a public rendezvous" as one of the three things excluded. The gap is therefore designed, documented, and now empirically load-bearing: **two cold peers that have both changed address cannot reach each other by any path in this codebase.** D1 priced its cost as "one pasted ticket, once per machine"; the observed cost recurs on every address change and, in the both-cold case, is unpayable.

### E2 — The join ticket is unsigned, for a reason that automation destroys

`JoinTicket` is `{issuer: PeerId, endpoints: Vec<Endpoint>, protocol, expires_at}` (`domain/join_ticket.rs:29-35`). **There is no signature field.** Its doc is explicit that this is intentional: "redeeming one proves nothing about the issuer beyond the fact that someone published these endpoints" (`:26-28`).

That was sound because the *channel* authenticated it — a ticket arrives from a person you already trust, over a medium you already share. A rendezvous automates the channel and removes that authentication entirely. The artifact must therefore become self-authenticating, or the rendezvous becomes an unauthenticated source of dial targets. **This is the central change the requirement demands**, and it is a change to a shared contract, not an addition to a server.

### E3 — The requirement's crypto is achievable in a strictly stronger form than asked

The requirement says "first time registered identity needs to be validated", which implies trust-on-first-use at the rendezvous: a registry of who owns what, established by whoever registers first.

That is weaker than what this codebase already supports. `PeerId` **is** the Ed25519 public key, constructible only from bytes that decode to a valid key (`shared_types/src/peer_id.rs:24-30`). A registration signed by the matching secret is therefore verifiable by anyone holding nothing but the registration — no prior state, no first-registration privilege, no owner table.

The consequence is worth stating plainly, because it removes work rather than adding it: **the rendezvous needs no identity state at all.** There is nothing to establish on first contact, nothing to back up, nothing whose loss or corruption could hand an identity to the wrong key. Every registration proves key ownership, not just the first one.

### E4 — Impersonation is already impossible, which bounds the damage before any design

A dialer authenticates the remote against its `PeerId` during the libp2p handshake (D2). A rendezvous serving false endpoints for a real peer produces **failed dials, not impersonation**.

So the signature buys something narrower than "stops impersonation", and the narrower thing is the real threat: without it, anyone can **fabricate registrations** — mint unlimited identities, fill the answer set with peers that do not exist, and make bootstrap fail while the rendezvous looks populated and healthy. Cheap to do, hard to distinguish from a quiet network.

### E5 — S1 is enforced by tests, and two of them collide with this work

`net_libp2p/src/serverless_integrity_test.rs` reads `cargo tree -e normal` for this crate and for `app`, failing the build if any of five crates enters the graph. **`libp2p-dns` is forbidden by name**, on the reasoning that a DNS transport "makes a `/dnsaddr` bootstrap list readable".

`app/src/cli/launch_options_test.rs:94-110` asserts that `--bootstrap`, `--bootstrap-node`, `--relay-address` and a list of role flags all stay *unparseable*, with the reasoning: "the moment one parses, this instance is no longer the same program as every other instance". It explicitly permits `--listen` and `--external-address` because those name addresses this peer *binds* or *is reached at*, never hosts it contacts.

A `--rendezvous <multiaddr>` option is precisely the shape that list exists to catch. This is a real, deliberate guard weakening — not a technicality.

One guard, however, is **not** in conflict: the role-flag prohibition is satisfied by putting the rendezvous in its own binary (`src/apps/rendezvous/` per the target layout) rather than behind a `--server` flag on the TUI. The TUI guard stays green and keeps meaning exactly what it says.

### E6 — `PeerDiscoveryPort` already permits what is being asked

Its doc reads: "An implementation that contacted a **default** rendezvous, bootstrap, or STUN server would satisfy the signature and violate the requirement" (`ports/peer_discovery_port.rs:16-18`). The word *default* is load-bearing and already carves out a configured one. This needs a precision edit, not a reversal — evidence the original design anticipated this seam.

### E7 — Precedent for every mechanical piece already exists

- **Signing over a pinned byte layout** — `Envelope::signable_bytes` in `shared_types`, with `EnvelopeSignerPort`/`EnvelopeVerifierPort` in `identity` and the secret never crossing the port (S3a).
- **A periodic background publisher** — `app/src/composition/heartbeat_beacon.rs`.
- **A bounded store with an eviction rule** — `PeerRoster`'s `MAX_PEERS = 1024` (S6).
- **Extending the wire without breaking peers** — `PayloadKind::from_code` maps unassigned codes to `Unknown` (S2).

Nothing here needs a new mechanism. The work is contract and boundary design, not machinery.

### E8 — What actually broke is address *staleness*, not the absence of a server

Worth naming because it shapes the solution: the cache does not fail because it is a cache, it fails because its entries have moved. A rendezvous helps only insofar as it is a peer whose own address is **stable** and whose registrations are **fresh**. That splits the design into a durable part (the rendezvous address, configured once) and a perishable part (registrations, short-lived and refreshed).

In those terms: **a rendezvous address is a join ticket that does not expire and does not depend on one particular peer being awake.**

## 2. Outcome

A peer whose cached addresses have all gone stale, with no LAN neighbour and no human available to paste a ticket, rejoins the network unaided — by asking a rendezvous it was configured with once. Every registration it receives proves ownership of the key it names. A peer with no rendezvous configured behaves exactly as it does today, and no rendezvous address ships with the software.

## 3. Acceptance criteria (proposed)

| # | Criterion |
| --- | --- |
| A1 | No rendezvous address is compiled in, defaulted, or bundled; a fresh install with no configuration walks exactly the three rungs it walks today, and this is asserted by test |
| A2 | A rendezvous rung sits between LAN and ticket, is attempted only when the free rungs produced nothing, and reports itself in `JoinDiagnostic` like every other rung |
| A3 | A registration is accepted only if it carries a signature verifying against the `PeerId` it names; an unsigned or mis-signed registration is discarded, not merely ranked lower |
| A4 | The rendezvous holds no identity-ownership state: verification uses the registration alone, so wiping its storage costs availability and never identity |
| A5 | A hostile rendezvous can withhold and can lie, and no lie it tells causes a peer to accept an identity; a fabricated registration fails at the handshake and is reported honestly |
| A6 | Registrations expire and are refreshed while the peer is online; an expired registration is not served |
| A7 | The rendezvous learns which peers are online and at what addresses; it does **not** learn who is looking for whom |
| A8 | More than one rendezvous may be configured, and losing any one of them does not isolate the peer |
| A9 | The rendezvous is a separate composition root; the TUI binary gains no role flag |
| A10 | The server bounds what it will store and states the bound and its eviction rule |
| A11 | The user can see that this peer is using a rendezvous, and which one — the privacy cost is visible in the interface, not only in documentation |
| A12 | No existing guard is weakened without a strictly stronger replacement assertion |

### Exclusions

Relaying message traffic — a rendezvous forwards no envelopes, and circuit relay stays peer-provided per D2. Store-and-forward and offline delivery (still excluded by `0002`). Looking up a *named* peer (see §6.3). Any default, bundled, or suggested-in-code rendezvous address. Rendezvous instances discovering other rendezvous instances. The `src/apps/` directory migration itself, which is steps 1–3 of the target layout and independent of this work.

## 4. Domain analysis

**Owning context: `membership`.** Bootstrap, tickets, endpoints and the roster are all its. The new rung, the registration artifact and the discovery port extension belong there.

**`identity` supplies verification only.** It already owns the signer and verifier ports and the rule that no secret crosses a port (S3a).

**The cross-context seam is the main design question.** Membership must not import identity. The `Envelope` precedent resolves it — pinned signable-bytes layout in `shared_types`, ports in `identity`, wiring in the composition root — but there are two shapes, and they are not equivalent:

- **(a) A registration is an `Envelope` with a new `PayloadKind`.** Reuses the codec, S2 versioning, and both existing ports wholesale. Cost: envelopes carry author and sequence semantics that a registration has no use for, and `DeliveryIndex` would need to be kept away from a payload kind that is not a message — a hazard this project has already been bitten by once, when heartbeats had to be rewired to an unwrapped transport to stay out of the delivery index.
- **(b) A parallel signed type in `shared_types` with its own pinned layout**, plus a membership-owned verifier port implemented over identity's key material in the root. More surface, no semantic smuggling.

**Ubiquitous language.** *Registration* (a signed, expiring statement that a peer is at these endpoints) is new and needed — it is not a ticket, because nobody hands it to anybody, and not an announcement, because it is durable and signed. *Rendezvous* is new. *Rung*, *endpoint*, *ticket*, *issuer* are unchanged.

**Invariants to state.** A registration's signature verifies against the `PeerId` it names, or it does not exist (unrepresentable, following the `PeerId` precedent). A rendezvous is never an authority on identity. The ladder is complete without any rendezvous configured.

**Commands / queries / ports.**
- Command: publish this peer's registration (periodic, while online).
- Query: fetch candidate registrations from a rendezvous.
- `PeerDiscoveryPort` gains the rendezvous rung's read; the publish side is closer to `announce` and may extend it rather than joining it.
- `BootstrapRung` gains a variant — a change to a `const LADDER` array whose doc currently argues no fourth rung may exist.
- Adapter work is `net_libp2p`: a request/response protocol, which the stack already carries.

**The rendezvous server itself** should be a full peer — its own `PeerId`, speaking the same libp2p request/response transport. It then needs no HTTP stack, no TLS certificates, and its multiaddr carries `/p2p/<PeerId>`, so clients authenticate *it* too and cannot be MITM'd onto a substitute. This also matches the user's framing of "instances of the app": a role, not a tier.

## 5. Risks

**The S1 amendment is the headline.** It touches `0002` §1 exclusions, D1, S1, AC4, and `BootstrapRung`'s doc. It must go through `$spdd-prompt-update` with the four preservation properties (§0 constraint) written as testable text, not as intent.

**The DNS collision is the sharpest technical conflict.** A stable rendezvous address wants a name, and `libp2p-dns` is forbidden by name in a passing test. Neither option is free: IP-only keeps the guard but re-introduces the exact staleness disease for the rendezvous itself, whose operator's address then cannot change without breaking every client; enabling DNS amends a guard whose stated reason ("makes a `/dnsaddr` bootstrap list readable") is *precisely what this feature does on purpose*. This should be decided before the canvas, not inside it.

**Guard weakening.** `AGENTS.md` forbids weakening an assertion to make a test pass. The `--rendezvous` option requires editing a list of forbidden option names; the replacement must be strictly stronger — assert no default exists, and assert the zero-config three-rung walk — or the edit is a regression wearing a feature's clothes.

**Privacy is a new cost that did not previously exist.** Under D1 there was no one to disclose to. A peer using a rendezvous discloses its address set and its online times to whoever runs that host. This is not mitigable, only disclosable, which is why A11 makes it visible in the interface.

**Registration flooding.** Registrations are cheap — one signature each, unlimited identities. A bound alone does not help if a flood can evict real peers; the bound and its eviction rule need to be designed against an adversary, not against accident. This is a genuine open problem, not a detail to settle in passing.

**Single rendezvous = single operator who can censor.** Supporting a list costs little at design time and is the entire difference between "an operator can switch you off" and "an operator can inconvenience you". Given that this is exactly S1's purpose, plural should be the shape from the start rather than a follow-up.

**Testing.** `infra-sim-net` has no rendezvous fabric. Without one, the rung is tested only against a port fake and the server only in isolation — which would leave the interesting case (a hostile or lying rendezvous) unexercised. Building the fabric is part of the work, not adjacent to it.

**Compatibility.** If the pasted `JoinTicket` also becomes signed, existing tickets stop validating. Tickets live 24 hours, so the window is small, but it is an S2 wire question and must be answered deliberately.

## 6. Unresolved questions

1. **Envelope-with-new-`PayloadKind`, or a parallel signed type?** The cross-context seam. *`system-architect` decision; no recommendation offered here, as the delivery-index hazard and the surface cost pull in opposite directions.*
2. **DNS names or IP-only for rendezvous addresses?** The `libp2p-dns` collision. *Recommended: DNS, and amend the guard with its reason rewritten — IP-only reproduces the staleness problem one level up, which is the problem being solved.*
3. **Sample lookup, or lookup by name too?** Returning "any N live registrations" solves the motivating failure — first contact, then gossip and the DHT do the rest — and the rendezvous never learns who wants to reach whom. Looking up a specific `PeerId` is needed only when two particular peers are both cold and both mobile, and it hands the operator the social graph. *Recommended: sample only in v1.*
4. **Does the pasted ticket become signed too?** *Recommended: one signed artifact serving both channels — a human-pasted ticket gains nothing from being unsigned, and two near-identical types with different trust properties is how the wrong one gets used.*
5. **What bounds a rendezvous's storage, against a flooding adversary?** Open.
6. **Is the rendezvous a full peer?** *Recommended: yes — no new transport, no certificates, and clients authenticate the rendezvous itself.*

## 7. Specialist routing

`system-architect` — **required**, and before the canvas: question 1 (cross-context seam), question 2 (the guard collision), and review of the S1 amendment text, since S1 is the safeguard this project exists to keep.
`domain-modeler` — the registration type, the signature invariant, `BootstrapRung`, and the `shared_types` layout.
`application-handler` — the rung, the publish command, and `JoinDiagnostic` reporting.
`repo` — `net_libp2p` request/response protocol and the discovery adapter.
`spdd-executor` — the `src/apps/rendezvous/` composition root and the TUI's rendezvous disclosure (A11).
`test-writer` — the sim fabric for a lying rendezvous, and the strictly-stronger replacement assertions for the two amended guards.
