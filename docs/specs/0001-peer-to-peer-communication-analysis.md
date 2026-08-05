# SPDD Analysis — Serverless Peer-to-Peer Communication

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Source requirement (verbatim):** "this is a distributed communication system. every instance of the app is equal and can communicate with another through network. there should be no servers. user should just open the app and join the network right away."

---

## 1. Repository evidence

Facts established by reading the repository, not inferred:

- The workspace contains **no code**: no `Cargo.toml`, no `src/`, no `tests/`, no `docs/`. Only `AGENTS.md`, `CLAUDE.md`, `.codex/agents/`, `.agents/skills/`, `.claude/` mirror, `scripts/sync-claude-agents.py`, `.gitignore`.
- `AGENTS.md` is authoritative: Rust Cargo workspace, DDD, hexagonal, CQRS, TDD. One bounded context per crate under `src/contexts/<context>/`, shared contracts in `src/shared_types/`, technical implementations in `src/infrastructure/`, integration tests in `tests/integration/`.
- Dependency direction is fixed: domain imports neither ports nor adapters; application depends on domain + ports; adapters implement ports. **Contexts never import each other** — only published contracts in `src/shared_types/` or domain events.
- Domain and application tests must not touch network, clock, database, or external services. Integration tests must be deterministic.
- Specialist ownership is declared in `.codex/agents/*.toml`; max 6 concurrent agent threads per session (`.codex/config.toml`).
- Git history contains only scaffolding commits. There is **no existing capability to reuse and nothing to migrate.**

**Consequence:** the first canvas operation is workspace creation. There is no prior art in this repository constraining transport, persistence, or UI choices — every such choice below is a new decision, not a continuation.

---

## 2. What the requirement actually says, and what it does not

The requirement fixes four properties and leaves the product undefined.

**Fixed (non-negotiable, treat as invariants):**

1. **Symmetry** — every instance runs identical code and holds identical authority. No instance has a role another lacks.
2. **Direct peer communication** — instances exchange data with each other over a network.
3. **No servers** — no operator-run central infrastructure.
4. **Zero-setup join** — opening the app is sufficient to be a participant. No account, no registration, no configuration step.

**Undefined (must be resolved before a canvas can be written):** what is communicated (text / voice / files), who can be reached (LAN / internet), whether identity survives a restart, whether history is stored, and what the UI is.

### 2.1 The "no servers" constraint needs a precise reading

Taken literally, "no servers" is unsatisfiable: in any peer-to-peer system every instance *is* a server — it must listen on a socket to be reachable. The requirement is about **ownership and asymmetry**, not sockets. The workable reading:

> No participant depends on infrastructure operated by anyone other than the participants themselves. Every process runs the same code and can both initiate and accept connections.

Under that reading, three things remain genuinely at odds with "no servers," and each is a decision the user must make:

| Mechanism | Why it looks like a server | Serverless alternative | Cost of the alternative |
| --- | --- | --- | --- |
| **Bootstrap** — finding the first peer | A hardcoded bootstrap list points at long-lived hosts someone operates | LAN multicast (mDNS) — no host required | Reach limited to the local network segment |
| **NAT traversal** — reaching a peer behind a home router | STUN/TURN are operator-run services | LAN-only, or manual port forwarding, or IPv6 with permissive firewall | Internet-wide connectivity becomes unreliable or manual |
| **Offline delivery** — messaging a peer who is not running | A store-and-forward host is a server | Peers relay for each other, or delivery simply fails | Trust, storage, and privacy problems, or a weaker product |

**This is the single highest-leverage decision in the project.** LAN-only is fully serverless and honest; internet-wide is not, without admitting some infrastructure.

**Recommendation:** scope v1 to a **single LAN / local network segment** with mDNS discovery. It satisfies "no servers" with zero asterisks, satisfies "open the app and join right away" literally, and defers NAT traversal without designing anything that must later be undone. Internet reach becomes a second discovery adapter behind the same port.

---

## 3. Outcome and acceptance criteria

### 3.1 Target outcome

A user launches the application on a machine. Within seconds and without any configuration, they see the other running instances on their network and can exchange messages with them directly. Closing the app removes them from the network; no state is left behind on anyone else's machine that they did not send.

### 3.2 Acceptance criteria (proposed, testable)

| # | Criterion |
| --- | --- |
| AC1 | Launching an instance with no configuration file, no arguments, and no prior state results in a joined, usable node. |
| AC2 | Two instances started independently on the same network discover each other within a bounded interval (target: 5s) with no manual address entry. |
| AC3 | A message sent by peer A is received, authenticated, and displayed by peer B without traversing any third process. |
| AC4 | Every instance runs one binary with one code path — no `--server` / `--client` flag, no role election, no privileged node. |
| AC5 | Stopping any single instance leaves every other instance functional; the remaining peers observe it leave within a bounded interval. |
| AC6 | Every message received is cryptographically attributable to the sending peer's identity; a message with an invalid or absent signature is rejected and never displayed. |
| AC7 | Duplicate delivery of the same message is idempotent — the message appears exactly once. |
| AC8 | Messages from a single author are displayed in that author's send order, regardless of arrival order. |
| AC9 | A peer identity is stable across restarts of that instance (subject to §5, Q4). |
| AC10 | Domain and application tests pass with no network, no real clock, and no filesystem. |

### 3.3 Explicit exclusions (proposed for v1)

Voice and video; file transfer; offline/store-and-forward delivery; internet-wide discovery and NAT traversal; group key management and forward secrecy; message editing and deletion; multi-device identity; mobile platforms; moderation beyond a local blocklist.

Each exclusion is a scope proposal, not a technical refusal — flag any the user wants pulled into v1 before the canvas is written.

---

## 4. Domain analysis

### 4.1 Ubiquitous language

| Term | Meaning | Notes |
| --- | --- | --- |
| **Peer** | One running instance of the application | The unit of symmetry; there is no "client" or "server" |
| **Local Peer** | The instance executing this process | Same type as any other Peer — the asymmetry is positional, not structural |
| **PeerId** | Stable, self-certifying identifier derived from a public key | Not assigned by anyone; cannot be claimed by another peer |
| **Display Name** | Human-readable label a peer announces | **Never unique, never identity** — collisions are legal and expected |
| **Endpoint** | Network location where a peer can currently be reached | Transient; a peer may change Endpoint without changing PeerId |
| **Network** | The set of peers mutually reachable at a moment in time | Has no membership list anyone owns; each peer holds only its own view |
| **Discovery** | The act of learning a peer exists and where to reach it | |
| **Presence** | A peer's observed liveness — online, stale, offline | Derived from evidence + elapsed time, never asserted by a third party |
| **Session** | An authenticated, encrypted, bidirectional link between two peers | |
| **Envelope** | The signed, versioned wire unit carrying a payload between peers | The compatibility contract |
| **Message** | An authored communication with content and a place in its author's order | |
| **Conversation** | The scope a message belongs to | Shape undecided — see Q5 |
| **Sequence Number** | Per-author monotonic counter | Basis of ordering and duplicate detection |
| **Blocked Peer** | A peer whose traffic the local peer refuses | Purely local decision; no global effect |

Two naming traps to settle now: do not use "Node" and "Peer" interchangeably (pick **Peer**), and do not let "user" enter the domain — there is no user account, only a peer.

### 4.2 Proposed bounded contexts

Three contexts, chosen so the "contexts never import each other" rule survives contact with this problem.

**`identity`** — the local peer's cryptographic identity and its judgments about other peers.
Owns: keypair lifecycle, PeerId derivation, display name, trust state (unverified / verified / blocked), first-contact policy.
Rationale: identity has no dependency on connectivity and is the root of every authenticity guarantee.

**`membership`** — who is out there and whether they are reachable.
Owns: the roster of known peers, discovery results, presence and liveness expiry, session lifecycle rules, join/leave, network status.
Rationale: discovery and presence share one aggregate (the peer roster) and splitting them would produce two contexts that cannot function apart.

**`messaging`** — authored communication.
Owns: messages, conversations, authorship, per-author causal ordering, deduplication, delivery state.
Rationale: distinct lifecycle and distinct invariants from membership; a message outlives the session that carried it.

**Boundary hazard to state explicitly in the canvas:** `messaging` needs to send to a peer, and `membership` knows who is reachable. Importing `membership` from `messaging` is the obvious and forbidden shortcut. The correct shape is an outbound `MessageTransportPort` owned by `messaging` whose adapter uses the transport infrastructure, plus `PeerConnected` / `PeerDisconnected` events published through `src/shared_types/` carrying only `PeerId`. `messaging` must never learn what an `Endpoint` is.

**`src/shared_types/`** publishes: `PeerId`, `ProtocolVersion`, the `Envelope` contract, and the cross-context peer lifecycle events. Nothing else — every addition here couples all three contexts.

**`src/infrastructure/`** holds: the transport crate (QUIC or TCP + Noise), the discovery crate (mDNS), the local store crate, and — importantly — a **deterministic in-process simulated network** used by integration tests.

### 4.3 Invariants

Domain-level, expressible as value objects and typed errors:

1. A `PeerId` is derived from a public key. Constructing one that does not match its key material is impossible by type.
2. A peer never accepts a session claiming its own `PeerId` (self-connection rejection).
3. Two concurrent sessions between the same peer pair collapse deterministically to one, by a rule both sides compute identically (e.g. lower `PeerId` keeps its outbound session). Simultaneous connect is the normal case in a symmetric network, not an edge case.
4. Every message has exactly one author, and that author is the peer whose key signed the envelope. Author is not a field a sender may set freely.
5. Sequence numbers are strictly monotonic per author. A gap means *not yet received*, never *lost*.
6. Applying the same message twice is a no-op — delivery is at-least-once, application is exactly-once.
7. Presence is derived: `online` requires evidence newer than a liveness window. No peer may assert another peer's presence as fact.
8. Display names never participate in identity, equality, addressing, or lookup.
9. The local peer's view is authoritative only for itself. There is no global state, and no operation may assume one exists.
10. An unverified signature is a rejection, not a warning — rejected content never reaches the read model.

### 4.4 Commands (mutate)

`InitializeLocalIdentity`, `SetDisplayName`, `VerifyPeer`, `BlockPeer`, `UnblockPeer` *(identity)*
`JoinNetwork`, `LeaveNetwork`, `RecordDiscoveredPeer`, `OpenSession`, `CloseSession`, `RecordPeerHeartbeat`, `ExpirePresence` *(membership)*
`SendMessage`, `AcceptInboundMessage`, `MarkMessageDelivered` *(messaging)*

`AcceptInboundMessage` is the critical one: inbound network traffic enters through an inbound port as a command, is validated in the application layer, and only then touches the domain. Adapters never construct domain aggregates from wire data directly.

### 4.5 Queries (read only)

`GetLocalIdentity`, `GetPeerTrustState`, `ListBlockedPeers` *(identity)*
`ListKnownPeers`, `ListOnlinePeers`, `GetNetworkStatus` *(membership)*
`ListConversations`, `GetConversationHistory`, `GetMessageDeliveryState` *(messaging)*

`GetNetworkStatus` should express `isolated | joining | connected(n)` — a serverless app must be able to tell the user "you are alone on this network," which is a normal state here, not an error.

### 4.6 Events (past tense)

`LocalIdentityInitialized`, `PeerVerified`, `PeerBlocked`
`NetworkJoined`, `NetworkLeft`, `PeerDiscovered`, `PeerConnected`, `PeerDisconnected`, `PeerPresenceExpired`
`MessageSent`, `MessageReceived`, `MessageRejected`, `MessageDuplicateIgnored`

`PeerConnected` / `PeerDisconnected` are the cross-context contract and belong in `src/shared_types/`. The rest stay context-internal.

### 4.7 Ports

Outbound: `PeerDiscoveryPort` (announce + observe), `PeerTransportPort` (dial / listen / send / close), `MessageTransportPort` (messaging's narrow view of transport), `IdentityKeyStorePort`, `EnvelopeSignerPort`, `EnvelopeVerifierPort`, `PeerDirectoryPort`, `MessageLogPort`, `ClockPort`, `EventPublisherPort`.

Inbound: `JoinNetworkPort`, `SendMessagePort`, `InboundEnvelopePort`, plus one query port per context.

`ClockPort` is not optional bookkeeping. Presence expiry, liveness windows, and retry backoff are all time-dependent, and AGENTS.md forbids real clocks in domain and application tests. Every time-dependent rule must read time through this port from day one — retrofitting it later means rewriting every presence test.

### 4.8 Adapters

mDNS discovery; QUIC (or TCP + Noise) transport; filesystem/embedded-store key store, peer directory, and message log; a UI adapter; and a **simulated in-memory network adapter** for tests — the last is production-grade infrastructure for this project, not a test helper, because it is the only way to get deterministic multi-peer tests.

---

## 5. Unresolved questions requiring a decision

Ordered by how much downstream design they control. Q1–Q3 block the canvas.

**Q1 — Network reach: LAN-only or internet-wide?**
Determines whether "no servers" holds absolutely. *Recommended: LAN-only for v1.*

**Q2 — What is communicated?**
Text messaging, voice, file transfer, or arbitrary payloads? These are different products with different transport requirements (voice needs unreliable low-latency datagrams; files need chunking and resumption). *Recommended: text messaging only for v1, with a payload type in the envelope so the transport does not have to change later.*

**Q3 — Conversation model.**
One shared broadcast channel everyone on the network sees; direct 1:1 conversations; or named rooms peers opt into? Each implies a different aggregate and a different membership question. *Recommended: one network-wide broadcast channel plus 1:1 direct messages. Named rooms raise "who decides who is in the room" — a hard problem without a server, best deferred.*

**Q4 — Identity persistence.**
Persistent keypair on disk (stable identity, requires local storage on first launch) or ephemeral per-launch (nothing to store, but a peer is a stranger every time)? Ephemeral conflicts with AC9 and makes trust decisions meaningless. *Recommended: persistent keypair generated silently on first launch — still zero-setup for the user.*

**Q5 — History persistence.**
Is conversation history stored locally, or is everything in-memory and lost on exit? *Recommended: in-memory for v1 behind a `MessageLogPort`, so a durable adapter drops in later without touching the domain.*

**Q6 — UI shell.**
TUI, CLI, native GUI, or web frontend? The repository has no UI stack and no precedent. *Recommended: a TUI — it exercises the full domain, keeps the adapter thin, and avoids a GUI toolkit decision now.*

**Q7 — Library policy.**
Is a P2P framework (`libp2p`, `iroh`) acceptable, or should transport be built directly on `quinn`/`tokio`? A framework supplies discovery, NAT traversal, identity, and gossip but is a large dependency that must be confined to adapters. *Recommended: build the LAN slice on `quinn` + `tokio` + an mDNS crate; the port boundary keeps a later framework swap cheap.*

**Q8 — Threat model.**
Passive observer on the same LAN, active MITM, or hostile peers attempting Sybil/spam? This sets the crypto and rate-limiting bar. *Recommended: assume passive observers and active MITM on first contact — mandatory transport encryption, mandatory envelope signatures, trust-on-first-use with out-of-band fingerprint verification available.*

**Q9 — Delivery guarantee promised to the user.**
Best-effort while both peers are online, or a retry/acknowledgement scheme? *Recommended: best-effort with explicit per-message delivery state shown in the UI. Silent failure is worse than a visible "not delivered."*

---

## 6. Risks

**Architectural**

- *`messaging` reaching into `membership`.* The most likely violation in this design, and it looks reasonable at the call site. Enforce via the port + shared event contract described in §4.2; make it a canvas safeguard, not a review hope.
- *Transport types leaking into the domain.* `SocketAddr`, `tokio` handles, `libp2p::PeerId`, and connection objects must stop at the adapter boundary. `Endpoint` is a domain value object with no `std::net` dependency.
- *Session establishment logic drifting into infrastructure.* Bytes on the wire belong in infrastructure; the rules about *which* session survives, *whether* a peer is accepted, and *when* identity is bound are domain rules in `membership`. Getting this split wrong makes the rules untestable.

**Security**

- *First-contact MITM.* Zero-setup join means no pre-shared trust. TOFU is the only option; the mitigation is a stable fingerprint the user can compare out-of-band, plus a visible unverified/verified distinction.
- *Address disclosure.* Presence announcement broadcasts the peer's IP to everyone on the network. This is inherent to serverless P2P and should be stated to the user, not silently accepted.
- *Sybil and spam.* Identities are free, so an attacker can mint unlimited peers. Without a server there is no global reputation. Mitigations are local: per-peer rate limits, local blocklist, message size caps.
- *Unbounded resource consumption from hostile peers.* Message size limits, per-session buffer caps, and connection count limits must exist from the first version — a symmetric open network has no gatekeeper to add them later.
- *Malformed input.* All validation happens at the adapter and application boundary. The domain must be unreachable by unvalidated wire data.

**Compatibility and operations**

- *Peers upgrade independently — there is no coordinated deploy.* Two versions of the app will meet on the same network on day one of the second release. The `Envelope` must carry a protocol version from the very first commit, and unknown fields and unknown message types must be handled by a defined rule (ignore vs. reject) chosen now. This is the most expensive thing to retrofit and the easiest to forget in a greenfield project.
- *No central observability.* Nothing aggregates logs or metrics. Diagnostics must be local and legible: network status, peer list with last-seen times, per-message delivery state, rejection reasons.
- *Nothing to migrate.* Greenfield. The only migration concern is local store schema versioning, which is cheap to establish now.

**Testing**

- *Nondeterminism is the default failure mode of this project.* Real sockets, real timers, and real concurrency produce flaky tests. AGENTS.md already forbids network and clock in domain and application tests, and requires deterministic integration tests; the simulated in-process network with a virtual clock and seeded randomness is how that requirement is met. Build it as an early operation, not as a late convenience.
- *Multi-peer scenarios need harness support:* N peers in one process, controlled message delivery order, injectable partitions, injectable delays. Reordering, duplication, and partition scenarios are core behavior here, not edge cases, and must be directly expressible in tests.
- *Presence and expiry are pure time logic* — fully unit-testable through `ClockPort` with no networking at all.

---

## 7. Specialist routing

| Area | Agent |
| --- | --- |
| Context boundaries, the `messaging`/`membership` decoupling, `shared_types` contract review | `system-architect` (read-only) |
| Aggregates, value objects, invariants, events, port traits, `shared_types` | `domain-modeler` |
| Command/query handlers, join/leave orchestration, inbound envelope handling, wiring | `application-handler` |
| Transport, discovery, key store, message log, simulated network | `repo` |
| Port fakes, virtual clock, multi-peer harness, reorder/duplicate/partition scenarios | `test-writer` |
| — | `api-designer` is **not** engaged: there is no HTTP surface in this design |

Max 6 concurrent threads per session per `.codex/config.toml`.

---

## 8. Recommended path into the canvas

Answer Q1–Q3 (and ideally Q4–Q9), then build the narrowest end-to-end slice that proves the hard part:

> Two instances on one LAN, started with no configuration, discover each other, establish an authenticated encrypted session, and exchange signed text messages that survive reordering and duplication — with the entire flow covered by deterministic tests over a simulated network.

That slice touches all three contexts, forces the protocol-versioning and clock-port decisions early, and produces no throwaway work if internet reach, richer conversation models, or persistence are added later.

**Assumption to confirm before the canvas:** every "Recommended" in §5 is my proposal from the requirement text, not a user decision. The canvas should not treat any of them as settled until confirmed.
