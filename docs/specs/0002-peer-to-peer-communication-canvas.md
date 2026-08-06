# REASONS Canvas — Serverless Internet-Wide Peer-to-Peer Text Messaging

**Status:** approved 2026-08-04 (D1, D2, D8/D9 explicitly confirmed by user; D5, D6, D7 per §9 defaults; D12 confirmed 2026-08-05). **Implemented through OP-12a as of 2026-08-05** — 1321 tests green, all four gates clean — **except the OP-13 two-machine smoke, which has not been run** (see OP-13's status block). Implementation source of truth, subordinate to `AGENTS.md`. Amendments are dated inline; nothing was relaxed silently.
**Input:** `docs/specs/0001-peer-to-peer-communication-analysis.md` plus user decisions: **Q1 = internet-wide reach, Q2 = text messaging only, Q3 = network-wide broadcast channel + 1:1 direct messages.**

**Decisions requiring user confirmation** are marked `⚠ CONFIRM` and collected in §9. The gravest is D1 (cold-start bootstrap): internet-wide reach with literally zero infrastructure makes the *first-ever* contact impossible without an out-of-band step. This canvas resolves it with a peer-generated **join ticket** and does not silently relax either "no servers" or "join right away" — see §3/D1 for the exact trade.

---

## 1. Requirements

### 1.1 Outcome

A user launches one binary with no configuration and no account. The instance assumes its persistent cryptographic identity (created silently on first launch), joins the network via any available bootstrap path (cached peers, LAN peers, or a pasted join ticket), and can immediately read and write a network-wide broadcast channel and hold 1:1 direct conversations with any reachable peer. All infrastructure functions — discovery, relaying, hole-punch coordination — are provided by the peers themselves. No participant depends on anything operated by a non-participant.

### 1.2 Acceptance criteria

| # | Criterion | Verified by |
| --- | --- | --- |
| AC1 | First launch with no config, no args, no prior state produces a working identity and a listening node; no registration step exists. | integration + manual smoke |
| AC2 | Two instances on the same LAN discover each other and connect within 5 s, unconfigured. | integration (sim + real smoke) |
| AC3 | An instance with a warm peer cache **or** one valid join ticket reaches the network over the internet within a bounded interval; failure produces a visible diagnostic, never a hang. | integration |
| AC4 | One binary, one code path, no role flags; every instance offers discovery, relay, and hole-punch coordination service to others. | code review + integration |
| AC5 | Stopping any instance leaves all others functional; peers observe the departure within the liveness window. | integration (sim) |
| AC6 | Every displayed message is signature-verified against the author's `PeerId`; invalid or unsigned envelopes are rejected before the read model and counted in local diagnostics. | unit + integration |
| AC7 | Redelivery of the same message (any transport path) changes nothing user-visible: exactly-once application over at-least-once delivery. **Scoped to a run: a user who clears their history re-arms application for what arrives afterwards** *(amended 2026-08-07, canvas `0013`)* — the dedup marks live in the conversation the clear discards, so a message already applied is applied again if it arrives again. Signature verification and author policy are unaffected, so what is re-armed is redundant display, never forged content. | unit + integration (sim dup injection) |
| AC8 | Messages from one author in one conversation display in that author's send order regardless of arrival order, **within the gap-tolerance window**; a message arriving after its gap was closed is reported, not displayed. *(Amended 2026-08-05, architect ruling.)* | unit + integration (sim reorder injection) |
| AC9 | `PeerId` is stable across restarts; the keypair persists locally and is created without user interaction. | integration |
| AC10 | Broadcast messages reach every online subscribed peer (eventual, gossip-propagated); no history replay to late joiners in v1 — **but a late joiner displays every message an author sends after it joins, within one gap-tolerance window of first contact.** *(Affirmative half added 2026-08-05: it is the clause the original code failed.)* | integration (sim) |
| AC11 | Each 1:1 message carries visible delivery state (`pending → delivered` or `failed`); silent loss is not a state. | unit + integration |
| AC12 | Two peers that cannot connect directly (NAT) communicate through a third *peer* acting as relay. | integration (sim topology) |
| AC12b | Relayed bytes are ciphertext to the relay. *(Split out 2026-08-05: no test at any layer can prove this — the sim fabric has no encryption and never hands the relay the frame, and even the manual smoke would need packet capture at the relay. The property is inherited from libp2p's Noise-over-circuit construction: relayed traffic runs TCP+Noise, so the circuit carries ciphertext.)* | code review + transport design |
| AC13 | Domain and application tests touch no network, clock, filesystem, or external service; multi-peer integration tests are deterministic (simulated network, virtual clock, seeded randomness). *(Amended 2026-08-05: **adapter-boundary** tests are exempt by necessity — `infra-net-libp2p`'s loopback tests and `app`'s `Node::start` smoke open real sockets. They skip when no socket can be bound, and `DISTRO_REQUIRE_NETWORK_TESTS=1` turns that skip into a hard failure so they cannot be silently vacuous on a machine that should have networking.)* | CI |
| AC14 | An envelope with an unknown **minor** addition (unknown fields / unknown payload kind) is tolerated per §7/S2; an unsupported **major** version is rejected with a logged reason. | unit |
| AC15 | An abandoned gap is visible in the conversation and counted in local diagnostics; a message arriving after its gap closed is reported, never silently discarded. *(Added 2026-08-05 — the inbound mirror of AC11.)* | unit + integration |
| AC16 | A peer that restarts continues to be heard by peers already online — its outbound sequence does not reset (D12). | integration (sim restart case) |

### 1.3 Exclusions (v1)

Voice/video; file transfer; offline store-and-forward delivery; broadcast history sync to late joiners; named/opt-in rooms; message editing/deletion; multi-device identity; mobile platforms; forward secrecy and group-key management beyond transport encryption; reputation systems (local blocklist only); any operator-run infrastructure.

---

## 2. Entities

### 2.1 `identity` context

| Entity | Kind | Content |
| --- | --- | --- |
| `LocalIdentity` | aggregate root | Ed25519 keypair handle, `PeerId`, `DisplayName`; signs envelopes, never exposes secret key bytes past its port |
| `PeerId` | value object (in `shared_types`) | Ed25519 public key; equality = key equality; renders as short fingerprint |
| `DisplayName` | value object | 1–64 Unicode scalar values, trimmed, control chars rejected |
| `TrustRecord` | aggregate | Per-remote-`PeerId` state machine: `Unverified → Verified`, orthogonal `Blocked` flag |
| `Fingerprint` | value object | Human-comparable digest of `PeerId` for out-of-band verification |

Events: `LocalIdentityInitialized`, `DisplayNameChanged`, `PeerVerified`, `PeerBlocked`, `PeerUnblocked`.

### 2.2 `membership` context

| Entity | Kind | Content |
| --- | --- | --- |
| `PeerRoster` | aggregate root | Known peers: `PeerId → (endpoints, presence, session state, last-seen)` |
| `Endpoint` | value object | Opaque multiaddress string + reachability class (`direct`, `relayed`); **no `std::net` types** |
| `Presence` | value object (derived) | **`Unknown`**, `Online`, `Stale`, `Offline`; computed from evidence age vs. liveness windows via `ClockPort`. *(Amended 2026-08-05 during OP-3: payload-free, since the evidence instant is an **input** to the derivation and remains readable as `KnownPeer::last_seen_at()`. Amended 2026-08-06 by canvas `0010`: `Unknown` means **no evidence has ever arrived** and is **not** `Offline` — the same rule as `0006`/S3 one layer down. It is not a rung on the Online→Stale→Offline ladder but the absence of a measurement, its only exit is evidence, and `Presence` therefore carries no ordering. `KnownPeer::last_seen_at` is `Option<Millis>`; `None` derives `Unknown`.)* |
| `PeerStanding` | value object (derived), **added 2026-08-06** | `Linked(Presence)` \| `Unlinked(Presence)` — the single classification the status count and the roster row are **both** computed from, so the two can no longer tell different stories about one peer. `Linked(Offline)` is a legitimate named state: the link is up and the peer is not answering. |
| `Session` | entity | Authenticated link to one peer: `Connecting → Established → Closed`, with direction |
| `JoinTicket` | value object | Self-describing bootstrap credential: issuer `PeerId` + endpoints + protocol version + expiry; string-encoded for copy/paste |
| `NetworkStatus` | value object | `Isolated`, `Joining`, `Connected(n)` — `Isolated` is a normal state, not an error |

Events: `NetworkJoined`, `NetworkLeft`, `PeerDiscovered`, `PeerConnected`†, `PeerDisconnected`†, `PeerPresenceExpired`. († published cross-context via `shared_types`, payload is `PeerId` only.)

### 2.3 `messaging` context

| Entity | Kind | Content |
| --- | --- | --- |
| `Conversation` | aggregate root | `ConversationId` = `Broadcast` \| `Direct(PeerId)`; holds per-author high-water marks, per-author `origin` (lowest committed sequence), buffered arrivals stamped with a **local** `received_at`, and the ordered message log. *(Amended 2026-08-05 per architect rule R.)* |
| `Message` | entity | `MessageId(author, conversation, seq)`, author `PeerId`, `MessageBody`, sent-at (author-claimed, display only), `DeliveryState` |
| `MessageBody` | value object | UTF-8 text, 1–16 KiB after trim |
| `SequenceNumber` | value object | Per `(author, conversation)` strictly monotonic `u64` |
| `DeliveryState` | value object | Direct: `Pending → Delivered` \| `Failed(reason)`; Broadcast: `Published` |

Events: `MessageSent`, `MessageReceived`, `MessageRejected`, `MessageDuplicateIgnored`, `MessageDeliveryStateChanged`.

### 2.4 `shared_types` published contract

*(Amended per system-architect OP-1 review, 2026-08-04 — enumerates the exact approved surface.)*
`PeerId` + `PeerIdError`, `Fingerprint` (rendering pinned by test), `ProtocolVersion { major, minor }`, `Compatibility` + its pure evaluation rule (§7/S2), `Envelope` + `EnvelopeSignature` + `PayloadKind`, cross-context events `PeerConnected` / `PeerDisconnected`. **Nothing else** — in particular, no port traits ever: `shared_types` is a data-contract crate, never a port host. Every addition couples all contexts and needs `system-architect` review.

Recorded decision (architect Note 2): `PeerId` accepts small-order ("weak") Ed25519 keys — a weak key only makes its *own* identity forgeable and harms no honest peer; excluding them is deferred beyond v1.

### 2.5 Invariants

1. `PeerId` construction is only possible from valid Ed25519 public-key bytes; identity–key mismatch is unrepresentable.
2. A session claiming the local peer's own `PeerId` is rejected (`SelfConnection` error).
3. Simultaneous connect between a peer pair collapses deterministically: the session initiated by the lexicographically **lower** `PeerId` survives; both sides compute this identically. This is the normal case, not an edge case.
4. A message's author is the `PeerId` whose signature verifies on the envelope — never a payload field.
5. *(Rewritten 2026-08-05 — architect rule R, resolving the invariant-5 vs. AC10 conflict.)* `SequenceNumber` per `(author, conversation)` is strictly monotonic. A gap means *not yet received* **for a bounded interval**: out-of-order arrivals buffer until contiguous, and a gap that neither closes within the gap-tolerance window nor fits the bounded buffer is **abandoned explicitly** — the log advances past it and emits `MessageGapClosed` naming the abandoned range and its cause (`ToleranceElapsed` | `BufferFull`). Content is never dropped silently, and never displayed out of its author's order. Sequence `FIRST` applies immediately (no settling delay at genesis). The rule is identical for `Broadcast` and `Direct`.
6. Applying an already-applied `MessageId` emits `MessageDuplicateIgnored` and changes no state. *(Tightened 2026-08-05:)* "already applied" means **actually applied**, never merely "at or below the high-water mark" — after a skip the two differ, and conflating them re-introduces silent loss. A message falling inside a closed gap is `Rejected(ArrivedAfterGapClosed)`, not a duplicate.
7. *(Rewritten 2026-08-06 by canvas `0010`, after live use found this violated in two places — the earlier certification of it as "unrepresentable" was wrong.)* `Presence` is derived from evidence age; a peer with no evidence is `Unknown`, **never `Offline`**. **Evidence is an act the peer itself performed**, observed here at approximately the time it happened, that no third party could manufacture: an inbound session open, a completed handshake, a frame arriving on a link with that peer (credited to the **carrier**, never the author), or that peer's acknowledgement of a direct request. **A third party's report is never evidence** — not a DHT record, not a gossip announcement, not an mDNS sighting, not a cache entry; a signed envelope authored by P but carried by Q is evidence about **Q only**, since a signed envelope is replayable and proves a past act, not a present one. **A completed handshake is evidence; a session that merely stays open produces none** — "the link has not been observed to fail" is not "the peer is alive". Only a peer that has produced evidence can expire. No peer asserts another's presence as fact.
7b. **The network status and the roster are one derivation.** A peer counted in `Connected(n)` is never rendered as absent. `Connected(n)` counts peers holding an **established session**, never peers judged live by evidence age — the two are different facts, and the interface shows both: the count is the fact, the presence is the derivation.
8. `DisplayName` never participates in identity, equality, addressing, or lookup.
9. Each peer's view is authoritative only for itself; no operation assumes global state.
10. Content failing signature verification never reaches any read model.
11. A `Blocked` peer's envelopes are dropped at the **content** boundary — the application boundary of every context that takes content, which in practice is `messaging` (`identity` owns the list; `membership` takes no envelopes). Blocking is purely local. *(Narrowed 2026-08-05 per reconciliation: a blocked peer may still carry gossip and still counts as a live link. `EnvelopeReceived{from}` names the **carrier**, not the author, and refusing a relay's evidence of life would break gossip for everyone. Refusing to relay for a blocked peer would change the network's behaviour, not just this peer's view — out of scope for v1.)*
12. Resource caps (S6) are enforced before deserialization wherever expressible.

---

## 3. Approach

Decisions with rationale and rejected alternatives. `⚠ CONFIRM` = needs user sign-off before the affected operation starts.

**D1 — Cold-start bootstrap: cached peers + LAN mDNS + out-of-band join ticket. `⚠ CONFIRM`**
Internet-wide discovery needs a first contact. Every mechanism that makes first-ever contact fully automatic (hardcoded bootstrap hosts, public rendezvous, DNS seeds) is operator-run infrastructure and violates "no servers" — the requirement the user ranked above convenience. Resolution: an instance bootstraps from, in order, (a) its cached peers from previous sessions, (b) mDNS-discovered LAN peers, (c) a **join ticket** — a string any member can produce and share out-of-band (chat, email, QR). After first join, tickets are never needed again on that machine. *Honest cost:* the very first internet join of a fresh install requires pasting one ticket; "open the app and join right away" holds unconditionally on LAN and on every subsequent launch. Rejected: hardcoded bootstrap nodes (servers); iroh-style relay+rendezvous infrastructure (servers); DHT-only (a DHT still needs a first peer).

**D2 — `rust-libp2p` as the P2P stack, confined to `src/infrastructure/net_libp2p`. ✅ CONFIRMED 2026-08-04.**
Internet reach requires NAT detection (AutoNAT v2), hole punching (DCUtR), peer relaying (Circuit Relay v2 — *peers* are the relays, which is exactly the serverless model), Kademlia DHT for peer routing, gossipsub for broadcast, and an encrypted multiplexed transport. *(Amended 2026-08-05: the canvas said "QUIC + Noise". libp2p's QUIC authenticates with TLS 1.3 and offers no Noise variant, so the delivered stack is **QUIC/TLS-1.3 preferred, TCP+Noise+Yamux as fallback**. Noise is what relayed circuits run over — which is exactly where AC12b's ciphertext-to-the-relay property lives.)* Hand-rolling this on `quinn`/`tokio` is a multi-quarter effort with high defect risk. Rejected: `iroh` (its connectivity model leans on n0-operated relay/discovery infrastructure — servers); hand-rolled `quinn` stack (scope); WebRTC (browser-oriented, still needs signaling). Containment rule: no `libp2p` type crosses the adapter boundary; `libp2p::PeerId` ↔ domain `PeerId` mapping lives in the adapter.

**D3 — Broadcast channel = one gossipsub topic; readable by every network member by design.**
"Network-wide broadcast" means public-to-the-network; gossip propagation gives eventual delivery without any hub. Rejected: naive flooding (bandwidth), per-peer fan-out from sender (sender becomes a de-facto hub and NAT-limited).

**D4 — 1:1 messages travel over the direct authenticated session (request/response protocol), E2E-encrypted by transport (Noise), signed at the envelope layer.**
When direct connection fails, the same stream runs through a peer relay; the relay carries Noise ciphertext (AC12). Broadcast messages are *not* confidential — they are signed but readable by all members (that is what a network-wide channel is). Rejected for v1: an additional message-layer encryption scheme with forward secrecy (excluded scope, revisit for rooms).

**D5 — Identity: persistent Ed25519 keypair, generated silently on first launch, stored locally. `⚠ CONFIRM`**
Satisfies AC9 and zero-setup simultaneously. Trust is TOFU with out-of-band `Fingerprint` comparison upgrading `Unverified → Verified` (analysis Q8). Rejected: ephemeral identity (makes trust and blocking meaningless, breaks AC9).

**D6 — Envelope encoding: CBOR via `ciborium`, named fields, unknown-field-tolerant. `⚠ CONFIRM`**
Self-describing named-field encoding is what makes S2's compatibility rule implementable. Rejected: `postcard`/bincode (positional — any field addition breaks old peers, fatal given uncoordinated upgrades); protobuf (workable alternative, heavier toolchain; switchable later behind the codec seam since `Envelope` is a `shared_types` contract).

**D7 — History: in-memory only, behind `MessageLogPort`. `⚠ CONFIRM`**
Conversation history dies with the process; a durable adapter is a later drop-in that touches no domain code. Peer cache and keypair *do* persist (D1, D5) — they are `membership`/`identity` concerns, not message history.

**D8 — UI: terminal UI (`ratatui`) as the only frontend, plus the composition-root binary. `⚠ CONFIRM`**
Exercises the full domain with the thinnest adapter; defers any GUI toolkit decision. Rejected for v1: web frontend (would drag in an HTTP surface and `api-designer` scope), native GUI.

**D9 — Composition root: new top-level binary crate `src/app/`. `⚠ CONFIRM` (structural addition to the AGENTS.md layout)**
The binary wires all three contexts + infrastructure; it belongs to no single context, so `contexts/<ctx>/bin/` misfits. `AGENTS.md`/`CLAUDE.md` must gain one line documenting `src/app/` when OP-12 lands — surfaced here rather than silently added.

**D10 — Delivery guarantee: best-effort with visible per-message state (AC11).**
*(Amended 2026-08-05, user-confirmed, after reconciliation found the promised retry cycle was never implemented.)* **One attempt**, then `Failed(reason)` — visible, reasoned, and resendable by the user. No retry loop, no backoff, no attempt counter, and therefore no timer the composition root must drive. `DeliveryFailure::RetriesExhausted` is retained as the name for "the attempt ended without acknowledgement" and its doc comment says so rather than implying a cycle. Rejected: unbounded retry queues (hidden store-and-forward — excluded), silent drop. AC11 is unaffected — the failure was always the visible part, never the retry.

**D13 — Heartbeats ride direct sessions, not the broadcast topic. Added 2026-08-06 (canvas `0010`).**
Liveness must not depend on gossip-mesh formation — that dependency is what produced the observed screen where two instances read `connected (2 peers)` while every roster row read `offline`. One signed `PayloadKind::Heartbeat` envelope goes to each peer holding an established session. **This loses nothing:** evidence is credited to the carrier, the carrier of any gossip message is a peer we hold a connection with, and the roster holds a session for essentially every libp2p connection — so the set of peers a broadcast heartbeat could ever produce evidence about was already a subset of the peers holding sessions. It also adds a **round trip**: the receiver gets `EnvelopeReceived{from}`, we get `DirectMessageDelivered{peer}`, so a healthy session yields mutual evidence every `HEARTBEAT_INTERVAL` and `Linked(Offline)` appears only when something is genuinely broken. Heartbeat correlation is kept structurally separate from message correlation — the beacon is wired to the unwrapped transport, so a heartbeat cannot enter the message delivery index; an unacknowledged heartbeat is a diagnostic counter, never a user-visible "message not delivered" notice, and never a negative presence claim. Rejected: broadcast heartbeats (the observed failure); counting session persistence as evidence (violates invariant 7, and is empirically false — there is no `ping` behaviour in the build).

**D11 — Time: every time-dependent rule (presence expiry, retry backoff, ticket expiry, dedup-window pruning, gap tolerance) reads `ClockPort`. Non-negotiable, echoed in S5.**

**D12 — The outbound sequence counter shares the keypair's lifetime. ✅ CONFIRMED 2026-08-05.**
Discovered during the OP-4 architect review: with in-memory-only history (D7), a restarted peer resumed at sequence 1 while online peers still held its high-water mark at N, so every message it sent was silently classified a duplicate — *a restarted peer went permanently mute*. `SequenceNumber` is specified per `(author, conversation)` but its state was per-process on both sides; its true domain of validity is the identity, not the process. Resolution: `messaging` declares its own `SequenceCounterPort` (per-`ConversationId` load-and-advance) in its `ports/`, implemented in OP-11 with the keystore's lifetime as its documented contract. If the key survives, the counter survives; if the key is gone the identity is gone and starting at 1 is correct. No wire change, no AC weakened. Narrowly qualifies D7 — a counter persists; conversation history still does not. Rejected: author epochs in the payload (nonce-derived epochs are unordered, weakening AC8 across epochs, and ordering them needs persistence anyway); full history persistence (schema, migration, pruning, and a privacy surface v1 excludes).

---

## 4. Structure

```text
Cargo.toml                          # workspace
src/shared_types/                   # crate: shared-types      (PeerId, ProtocolVersion, Envelope, PayloadKind, peer lifecycle events)
src/contexts/identity/              # crate: identity
src/contexts/membership/            # crate: membership
src/contexts/messaging/             # crate: messaging
    └── src/{domain,ports,application,adapters}/     # per AGENTS.md; contexts hold no bin/ (D9)
src/infrastructure/net_libp2p/      # crate: infra-net-libp2p  (transport, discovery, gossip, relay adapters)
src/infrastructure/store_fs/        # crate: infra-store-fs    (keystore, peer cache)
src/infrastructure/sim_net/         # crate: infra-sim-net     (deterministic in-process network + virtual clock; test infrastructure)
src/app/                            # crate: app               (composition root + TUI adapter + binary)   ⚠ D9
tests/integration/                  # deterministic multi-peer scenarios over infra-sim-net
docs/specs/                         # this canvas + analysis
```

**Dependency direction (violations are build-breaking design errors):**

- `shared_types` depends on nothing internal.
- Each context: `domain` → nothing internal but `shared_types`; `ports` → `domain` + `shared_types`; `application` → `domain` + `ports`; `adapters` → own `ports` (in-crate adapters only where trivial; real ones live in `src/infrastructure/`).
- `infra-*` crates depend on the context `ports` they implement + `shared_types`. Never on `application` — **with one carved-out exception**: `infra-sim-net` is *test* infrastructure whose job is to assemble whole contexts, so it depends on all three `*Context` types. Confined to `harness/sim_peer.rs`, and it is never linked into the production binary (`app` must not depend on it). `infra-net-libp2p` and `infra-store-fs` observe the rule strictly.
- Context-local value objects are duplicated rather than shared: each of `membership` and `messaging` defines its own `Millis`, `DurationMillis`, and `ClockPort`, and `identity`/`messaging` each define their own signer/verifier traits and `SignatureVerdict`. This is the correct consequence of "contexts never import each other" plus "`shared_types` hosts no port traits" — not an oversight. A single adapter implements both sides, so one object still serves both contexts.
- `app` depends on everything; nothing depends on `app`.
- **Contexts never import each other.** `messaging` ↔ `membership` interaction happens only via `PeerConnected`/`PeerDisconnected` (`shared_types`, carrying `PeerId` only) and `messaging`'s own `MessageTransportPort`. `messaging` must never learn what an `Endpoint` is.
- No `std::net`, `tokio`, `libp2p`, or `ciborium` type in any `domain/` or `application/` module. (`Envelope` in `shared_types` is a plain struct; codecs live in adapters.)

**Ports (owner → trait, all `Port`-suffixed):**

| Owner | Outbound | Inbound |
| --- | --- | --- |
| `identity` | `IdentityKeyStorePort`, `TrustRecordStorePort` (added during OP-5 — `TrustRecord` is an aggregate and needs persistence), `EnvelopeSignerPort`, `EnvelopeVerifierPort` | `IdentityCommandPort`, `IdentityQueryPort` |
| `membership` | `PeerDiscoveryPort` (announce/observe/ticket-redeem), `PeerTransportPort` (dial/listen/close), `PeerCachePort`, `ClockPort`, `EventPublisherPort` | `JoinNetworkPort`, `MembershipQueryPort`, `InboundSessionPort` |
| `messaging` | `MessageTransportPort` (send-direct/publish-broadcast, addressed by `PeerId` only), `MessageLogPort`, `AuthorPolicyPort` (**added 2026-08-05** — invariant 11 had no enforcement site; messaging's own trait, wired at the root to `identity`'s block list, never `shared_types`), `SequenceCounterPort` (**added 2026-08-05, D12** — per-`ConversationId` load-and-advance, keystore lifetime), `EnvelopeSignerPort` (messaging's **own** trait — amended 2026-08-05 after OP-2 surfaced that messaging could verify inbound envelopes but not produce signed outbound ones; same precedent as the verifier below, wired at the composition root to the one underlying signer, so invariant 4 holds on both directions), `EnvelopeVerifierPort` (messaging's **own** trait in its `ports/`, expressing only its need — verify an `Envelope` against its author; the composition root wires both contexts' verifier ports to the same underlying implementation; amended per architect Note 5 — a re-export from `identity` would be a cross-context import), `ClockPort`, `EventPublisherPort` | `SendMessagePort`, `InboundEnvelopePort`, `MessagingQueryPort`, `PeerLifecyclePort` (**added 2026-08-05** — the driving port the composition root calls to fan `PeerConnected`/`PeerDisconnected` into messaging for D10 fail-pending) |

Commands and queries stay in separate handler modules end-to-end (`application/commands/`, `application/queries/`); no handler both mutates and reads for return beyond its own result.

---

## 5. Operations

Ordered, independently verifiable. Every operation ends with: new/changed tests passing, `cargo test -p <crate>` green, `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean. TDD throughout — failing test first. Max 6 concurrent agent threads; the parallel groups below respect that.

**OP-0 — Bootstrap the Cargo workspace** *(spdd-executor)*
Workspace `Cargo.toml`; empty-but-compiling crates per §4 skeleton (no `app` yet); CI-ready: all four AGENTS.md checks green on the empty workspace. Verify: `cargo build --workspace` + all gates.

**OP-1 — `shared_types` contract** *(domain-modeler; system-architect reviews before merge)*
`PeerId` (validity invariant 1), `Fingerprint` rendering, `ProtocolVersion`, `Envelope` struct + `PayloadKind`, compatibility rule of S2 expressed as pure functions (`Envelope::compatibility(&ProtocolVersion) → Accept | Tolerate | Reject`), `PeerConnected`/`PeerDisconnected`. Tests: PeerId construction/rejection, version-rule truth table (drives AC14), fingerprint stability.

**OP-2 — `identity` domain + ports** *(domain-modeler — parallel group A with OP-3, OP-4)*
`LocalIdentity`, `DisplayName`, `TrustRecord` state machine, typed errors, the three outbound ports. Tests: trust transitions incl. block-while-verified, display-name validation, sign/verify round-trip through port fakes.

**OP-3 — `membership` domain + ports** *(domain-modeler — group A)*
`PeerRoster`, `Endpoint`, `Presence` derivation, `Session` lifecycle with invariant 3 collapse rule, `JoinTicket` (validity + expiry via clock value passed in), `NetworkStatus`, typed errors, ports. Tests: collapse-rule symmetry (both orderings, property-style), presence derivation truth table, self-connection rejection, expired/malformed ticket rejection.

**OP-4 — `messaging` domain + ports** *(domain-modeler — group A)*
`Conversation`, `Message`, `SequenceNumber` monotonicity + gap buffering (invariant 5), dedup (invariant 6), `DeliveryState` transitions, ports. Tests: reorder buffering until contiguous, duplicate no-op, author-from-signature only, body limits.

**OP-4a — `messaging` domain rework: rule R, block port, D12 counter port** *(domain-modeler — inserted 2026-08-05 by architect ruling; blocks OP-7's merge, not its start)*
Implement rule R in `Conversation`/`AuthorLog`: per-author `origin`, local `received_at` on buffered arrivals, `close_aged_gaps(now, tolerance)` pure sweep (buffer-full is the second trigger for the same close, emitting in `PeerId` order for determinism), `MessageGapClosed { conversation, author, from, to, cause }`, `RejectionReason::ArrivedAfterGapClosed` (remove the now-unreachable `OutOfOrderBufferFull`), `is_applied` by actual membership. Add `DurationMillis`, a `received_at` parameter distinct from `claimed_sent_at`, `Conversation::fail_pending(DeliveryFailure)`, and a rehydrate constructor. Declare `AuthorPolicyPort` and `SequenceCounterPort` (D12) in `ports/`. Tests: the architect's 13 domain cases incl. genesis-not-delayed, reorder inside vs. beyond the window, mid-stream permanent gap, buffer-full-closes-rather-than-rejects, no false diagnostics on ordinary reorder, idempotent sweep, determinism across authors, `Direct` parity, and the 1..=8 permutation property.

**OP-5 — `identity` application** *(application-handler — parallel group B with OP-6, OP-7; each starts when its domain op lands)*
Commands `InitializeLocalIdentity` (idempotent: load-or-create via keystore port), `SetDisplayName`, `VerifyPeer`, `BlockPeer`, `UnblockPeer`; queries `GetLocalIdentity`, `GetPeerTrustState`, `ListBlockedPeers`. Tests with port fakes only.

**OP-6 — `membership` application** *(application-handler — group B)*
Commands `JoinNetwork` (bootstrap ladder of D1: cache → mDNS → ticket), `LeaveNetwork`, `RecordDiscoveredPeer`, `OpenSession`/`CloseSession`, `RecordPeerHeartbeat`, `ExpirePresence` (clock-driven); queries `ListKnownPeers`, `ListOnlinePeers`, `GetNetworkStatus`; publishes `PeerConnected`/`PeerDisconnected`. Tests: full ladder incl. all-paths-fail → `Isolated` with diagnostic (AC3), presence expiry through fake clock, event emission on session change.

**OP-7 — `messaging` application** *(application-handler — group B)*
Commands `SendMessage` (direct + broadcast paths kept separate), `AcceptInboundMessage` (verify signature → check block state via own port contract → dedup → order → read model; rejection emits `MessageRejected`), `MarkMessageDelivered`; queries `ListConversations`, `GetConversationHistory`, `GetMessageDeliveryState`; reacts to `PeerDisconnected` by failing that peer's pending directs (D10). Tests: AC6/AC7/AC8/AC11 at unit level via fakes; blocked-peer drop (invariant 11).

**OP-8 — `infra-sim-net`: deterministic network + virtual clock + multi-peer harness** *(test-writer — may start with group B once OP-2..4 ports exist)*
In-process implementations of `PeerTransportPort`, `PeerDiscoveryPort`, `MessageTransportPort`, `ClockPort`; harness runs N peers in one process with scripted delivery order, injectable delay/duplication/reorder/partition, seeded RNG, manual clock advance. Never linked into the `app` binary. Tests: harness self-tests proving determinism (same seed → same trace).

**OP-9 — Integration scenarios over sim-net** *(test-writer)*
`tests/integration/`: two-peer discovery→session→DM round-trip; simultaneous-connect collapse; broadcast across 5 peers with gossip-order scrambling (AC10); duplicate + reorder injection (AC7/AC8); partition → presence expiry → reconnciliation (AC5); relay-topology DM where direct dial is scripted to fail (AC12 logical layer); blocked-peer traffic drop. All deterministic (AC13). Verify: `cargo test --workspace` green.

**OP-10 — `infra-net-libp2p` adapter** *(repo — parallel group C with OP-11)*
libp2p swarm: QUIC + Noise, identify, Kademlia, mDNS, AutoNAT, Circuit Relay v2 (server side always on — AC4), DCUtR, gossipsub topic, request/response protocol for directs; `ciborium` envelope codec (D6) with S2 tolerance behavior; S6 resource caps at this boundary; `libp2p::PeerId` ↔ `PeerId` mapping; ticket redemption = direct dial of ticket endpoints. Tests: codec round-trip + tolerance/rejection tables, mapping property tests, localhost two-swarm loopback integration test (fixed RFC 8032 **keys**; **ports are OS-chosen**, since a pinned port fails whenever anything else on the machine holds it; no external network). Plus `distro_behaviour_test.rs` asserting AC4 structurally — the relay **server**, AutoNAT server, `kad::Mode::Server`, and DCUtR are present and *offered to strangers*, read from the handler the behaviour would give an inbound connection, so a service that is constructed but disabled still fails.

**OP-11 — `infra-store-fs` + in-memory message log** *(repo — group C)*
Keystore (created `0600`-equivalent, load-or-create, D5), peer cache with schema-version header (S4), in-memory `MessageLogPort` (D7), trust-record store, `SequenceCounterPort` (D12). **Amended 2026-08-05:** also the production **signer/verifier** implementing both `identity`'s and `messaging`'s `EnvelopeSignerPort`/`EnvelopeVerifierPort` — a canvas gap found during OP-11: no operation had been assigned them, and OP-12 cannot sign an outbound envelope without one. They belong here because this is where the secret key lives; the port boundary keeps the key from crossing anywhere else. Tests: tempdir round-trips, corrupted/foreign-version file → typed error not panic, cache prune behavior.

**OP-12 — `app`: composition root + TUI** *(spdd-executor; requires D8/D9 confirmed)*
Wire real adapters to all three contexts; `ratatui` panes: broadcast, DM per peer, roster with presence + trust badge + fingerprint view, network status line (`Isolated/Joining/Connected(n)`), ticket generate/redeem flow, block/verify actions, per-message delivery marks. Update `AGENTS.md` + `CLAUDE.md` with the `src/app/` line (D9).

*(Amended 2026-08-05 to record what the root actually owns, all of it load-bearing and none of it previously named here.)* The root also supplies: a **system `ClockPort`** implementing both contexts' traits over UNIX-epoch millis advanced monotonically (the shared origin is what makes a `JoinTicket` expiry meaningful across machines); the **`TrustDirectory`** adapting `identity`'s block list into `messaging`'s `AuthorPolicyPort` (invariant 11); the **`CorrelatingTransport`** mapping `EnvelopeSignature` → `MessageId` so an asynchronous delivery report can name its message; a **`HeartbeatBeacon`** sending signed empty `PayloadKind::Heartbeat` envelopes, since OP-10 emits no liveness probe by design; and a **`TickSchedule`** driving four clock duties — presence expiry and heartbeat at `LivenessWindows::HEARTBEAT_INTERVAL`, `close_aged_gaps` at `GAP_TOLERANCE / 4`, and a trust refresh. Without those ticks AC5 never fires and gaps on quiet conversations never close.

Tests: TUI view-model unit tests; a `Node::start` smoke against a tempdir profile and loopback config (the canvas originally specified "contexts assemble against sim-net in-process", which is **impossible inside `app`** — it must never link `infra-sim-net`; the equivalent assembly proof lives one crate over in `SimPeer::assemble`).

**OP-12a — Close the AC11 asynchronous-failure hole** *(application-handler — inserted 2026-08-05, found during OP-12)*
`send_direct` returns `Ok` once the request is queued; a later refusal or timeout arrives as `NetworkEvent::DirectMessageFailed`. With the session still up, nothing can move that one message to a terminal state: `message_delivered` is the wrong direction and `peer_disconnected` fails *every* pending direct to that peer. The message stays `Pending` forever, which is precisely the silent-loss shape AC11/D10 forbid. The domain already has `Conversation::mark_failed(&MessageId, DeliveryFailure)`; what is missing is the application command and the inbound port method exposing it. Add `MarkMessageFailed` and `InboundEnvelopePort::message_delivery_failed(id, DeliveryFailure)`, then wire `DirectMessageFailed` to it in the composition root. Tests: an asynchronously refused direct reaches `Failed(reason)` while its session stays up and its peer's other pending directs are untouched; an unknown/duplicate correlation is a typed error, not a panic.

**OP-13 — Real-network smoke + canvas reconciliation** *(spdd-executor, then `$spdd-sync`)*
Manual protocol: two machines, LAN join (AC2), ticket join across networks (AC3), NAT-relay path check (AC12) — results recorded in PR description as evidence, not as CI. Full gate run: all four AGENTS.md checks. Then run `$spdd-sync` against this canvas; drift is surfaced, never silently absorbed.

**Status 2026-08-05 — PARTIALLY COMPLETE.**
- ✅ Gates: `cargo test --workspace` 1321 passed / 0 failed, `cargo build --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` all exit 0; `scripts/sync-claude-agents.py --check` in sync.
- ✅ `$spdd-sync` reconciliation performed; its findings are the dated amendments throughout this canvas, plus four new automated checks (AC4 structural assertions, `Node::start` smoke, S1 dependency-graph guard, loud network-test skip).
- ❌ **The two-machine smoke has NOT been run.** It requires two physical machines on different networks and cannot be performed from this environment. `src/app/README.md` documents a **one-machine, two-profile** procedure which exercises the ticket rung, the warm-cache rung, and port pinning — but *cannot* produce evidence for AC2 (real mDNS on a real interface), AC3 across actual networks, or AC12 (a real NAT, where DCUtR + AutoNAT v2 + Circuit Relay v2 are exercised together). The two-machine protocol still needs writing before it can be run.
- ❌ Consequently unproven outside a type checker: that mDNS binds multicast on a real interface (`DistroBehaviourError::MdnsUnavailable` is unreachable in tests), that AC2's 5-second bound holds against a real QUIC handshake (currently arithmetic on a virtual clock), and that the ratatui terminal path behaves on a real TTY beyond the pty smoke already run.

**This canvas is therefore NOT yet marked IMPLEMENTED.** Everything provable without two machines is proven; what remains is exactly the manual protocol above.

---

## 6. Norms (by reference)

- `AGENTS.md` — Project Structure & Architecture: layout, inward dependencies, no cross-context imports.
- `AGENTS.md` — Build/Test commands: the four gates; narrowest check per change, all four before declaring done.
- `AGENTS.md` — Coding Style: `Port` suffix, intent-named handlers, imperative commands, past-tense events, one principal implementation per file, no `utils.rs`.
- `AGENTS.md` — Testing: red-green-refactor, co-located `module_test.rs` with `#[cfg(test)] mod module_test;`, port fakes for domain/application, deterministic integration tests, regression test per bug fix, never weaken an assertion.
- `AGENTS.md` — Codex Workflow: route by `.codex/agents/` ownership, ≤6 concurrent threads, reconcile before verification.
- `AGENTS.md` — Commits/PRs and Security & Configuration: imperative focused commits; no credentials; validation at adapters; secrets outside domain crates.

---

## 7. Safeguards (non-negotiable; changing any requires `$spdd-prompt-update`, never silent relaxation)

**S1 — Serverless integrity.** No operator-run host may enter any code path: no hardcoded bootstrap addresses, no default relay/rendezvous/STUN endpoints, no telemetry endpoints. Peers provide all services (AC4). Any dependency default that phones home is disabled in the adapter.

**S2 — Wire compatibility from the first commit.** Every envelope carries `ProtocolVersion{major, minor}`. Same major: unknown fields and unknown `PayloadKind`s are ignored with a local diagnostic counter. Different major: reject with logged reason. The rule lives in `shared_types` as pure functions (OP-1) and every codec obeys it. Peers upgrade independently — there is no coordinated deploy, ever. *(Architect Note 4, 2026-08-04:)* the signable-bytes layout is pinned per major version, so additive minor evolution must ride inside `payload` or as new `PayloadKind`s; a security-relevant envelope-level field requires a major bump.

**S3a — The transport key seam (recorded 2026-08-05, found during OP-10).** The libp2p Noise/TLS handshake needs the Ed25519 secret itself, so it cannot be delegated behind a port. The invariant stands as written — *the port* never exposes secret bytes, and `IdentityKeyStorePort` still returns only a `PeerId`. The transport identity is obtained by the composition root through a narrow, explicitly-named method on the **concrete** `FileIdentityKeyStore` (not the port), passed straight into `NetworkIdentity::from_ed25519_secret_key`, and never logged, returned, or stored elsewhere. Two consumers of the secret exist by design — the signer and the transport handshake — and both live in infrastructure beside the key.

**S3 — Boundary validation.** Wire data reaches the domain only through `AcceptInboundMessage`/`InboundSessionPort` after size, signature, version, and block checks in adapter + application layers. Adapters never construct domain aggregates from raw bytes.

**S4 — Local-store migration discipline.** Keystore and peer-cache files carry a schema version from v1; an unknown version is a typed error with a preserved original file, never a destructive rewrite.

**S5 — Determinism.** All time via `ClockPort`; all randomness in testable paths seedable; domain/application tests free of network/clock/filesystem (AC13). The sim-net harness is the required vehicle for every multi-peer behavior claim.

**S6 — Hostile-input caps from v1.** *(Amended 2026-08-05: the original text said all caps are "enforced in `infra-net-libp2p`". Three are domain rules and correctly live in the domain — an adapter would have been the wrong home.)* All are constants with rationale comments. A symmetric open network has no gatekeeper to add them later.

- **Domain (`membership`, added 2026-08-06)**: `PeerRoster::MAX_PEERS` — never-heard-from peers cannot expire, so the roster needs a cap with a stated eviction rule: **evict `Unknown` with no session first, oldest recorded first; never evict an entry with a session or with evidence.** A full roster is a *state*, not an error — once at capacity every further sighting returning `Err` would be an attacker-inducible diagnostic flood. **The peer cache holds only peers that produced evidence** — the chain mDNS/Kademlia → roster → cache → next launch's *first* bootstrap rung means an unfiltered cache writes attacker-supplied identities to disk and dials them ahead of the LAN.
- **Adapter (`infra-net-libp2p`, added 2026-08-06)**: `max_observed_peers` = 256 with a 15-minute sighting retention — reading mDNS sightings must not consume them (a destructive read left the LAN bootstrap rung working exactly once per process), but sightings are untrusted network input and cannot accumulate without bound.
- **Domain (`messaging`)**: `MessageBody::MAX_BYTES` = 16 KiB; `AuthorLog::MAX_BUFFERED_MESSAGES` = 64 (the per-author *gap* buffer); `Conversation::GAP_TOLERANCE` = 2000 ms (above WAN gossip reorder spread, below the human "is this broken?" threshold).
- **Adapter (`infra-net-libp2p`, `ResourceLimits`)**: envelope ≤ 32 KiB checked *pre-deserialization*; ticket ≤ 4 KiB; per-peer inbound rate limit; `max_session_buffered_messages` = 256 (the per-connection *gossip* queue — a different cap from the domain's 64, against libp2p's 5000/connection default); `max_messages_per_rpc` = 16 (libp2p's default is unlimited); max concurrent sessions and per-peer connections; relay circuit bandwidth, duration, and count caps.

**S7 — Known connectivity limit, stated not hidden.** If no currently-online peer is publicly reachable, two symmetric-NAT peers cannot connect — inherent to serverless P2P. The UI must be able to say so (`Failed(NoRelayAvailable)`), and README/docs state it. Likewise: LAN/subsequent joins are zero-step; the first-ever internet join needs one ticket (D1).

**S8 — Privacy disclosure.** Joining announces the peer's addresses to the network and broadcast messages are network-public; both stated in user-facing docs. *(Amended 2026-08-07, canvas `0013`.)* The original sentence continued "No additional data leaves the machine", and the clipboard makes it false: a join ticket carries this peer's `PeerId` and endpoint list, and copying one exposes them to every process that can read the clipboard, to any clipboard-history manager, and — on macOS Universal Clipboard, Windows cloud clipboard and several Linux managers — to another device over a network the user did not choose. The safeguard now reads: **the only data this build puts anywhere the user did not point it is a join ticket the user explicitly copied, and that disclosure is stated in `--help` and in the help overlay.** Nothing is sent anywhere on its own; there is still no telemetry, no analytics, and no endpoint this build contacts that a peer did not name. Rejected: keeping the absolute sentence and refusing the clipboard (the manual copy it forces is the friction the feature exists to remove), and keeping the sentence while shipping the clipboard anyway (a safeguard that is false is worse than one that is narrow).

---

## 8. Agents

| Operation | Agent | Group |
| --- | --- | --- |
| OP-0 workspace | `spdd-executor` | serial |
| OP-1 shared_types | `domain-modeler` (+ `system-architect` review) | serial |
| OP-2 / OP-3 / OP-4 domain | `domain-modeler` ×3 | A (parallel) |
| OP-5 / OP-6 / OP-7 application | `application-handler` ×3 | B (parallel) |
| OP-8 sim-net, OP-9 integration | `test-writer` | B/serial after B |
| OP-10 libp2p, OP-11 stores | `repo` ×2 | C (parallel) |
| OP-12 app + TUI | `spdd-executor` | serial |
| OP-13 smoke + sync | `spdd-executor` → `$spdd-sync` | serial |

`system-architect` additionally reviews any proposed `shared_types` addition and the OP-7/OP-6 seam before group B merges. `api-designer` is **not engaged** — no HTTP surface exists (D8). Concurrency never exceeds 6 (`.codex/config.toml`); groups A, B, C each ≤ 4 active threads. Results of every parallel group are reconciled (contract check across the seam + full workspace gates) before the next group starts.

---

## 9. Open confirmations

| # | Decision | Default if unconfirmed |
| --- | --- | --- |
| D1 | Join-ticket cold start (first-ever internet join needs one pasted ticket) | **Blocks OP-6** — no serverless alternative exists; explicit sign-off required |
| D2 | `rust-libp2p` dependency | Blocks OP-10; groups A/B unaffected |
| D5 | Persistent keypair on disk | Proceed (required by AC9) |
| D6 | CBOR/`ciborium` envelope encoding | Proceed; codec is swappable behind OP-1 contract |
| D7 | In-memory-only history | Proceed (port-isolated) |
| D8 | TUI (`ratatui`) frontend | Blocks OP-12 only |
| D9 | `src/app/` composition crate + AGENTS.md line | Blocks OP-12 only |

Assumptions embedded without a decision letter: liveness window, retry counts, and cap constants in S6 are engineering defaults to be set with rationale comments during their operations; none is user-visible policy.
