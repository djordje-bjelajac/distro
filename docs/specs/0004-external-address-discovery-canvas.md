# REASONS Canvas — External Address Discovery (Piece 1 of 3)

**Status:** **IMPLEMENTED 2026-08-05.** OP-1 complete; all eight acceptance criteria proven by 23 new tests (13 ledger, 9 driver, 1 loopback); `cargo test -p infra-net-libp2p` 144 passed, and all four workspace gates exit 0, including under `DISTRO_REQUIRE_NETWORK_TESTS=1` with zero skips. Amendments from the reconciliation are marked inline and dated; D4's mechanism was materially wrong and is corrected below. Subordinate to `AGENTS.md` and to `0002-peer-to-peer-communication-canvas.md` (the system canvas), which it extends without amending.
**Input:** `docs/specs/0003-external-address-discovery-analysis.md`.
**Scope:** `src/infrastructure/net_libp2p/` only. No domain, application, or context change.

---

## 1. Requirements

### Outcome

A peer that is genuinely reachable from outside its NAT learns that address from the peers that can see it, corroborates it, and advertises it. Its join tickets, DHT records, and announcements then carry an address a stranger can dial.

### Acceptance criteria

| # | Criterion | Verified by |
| --- | --- | --- |
| P1-1 | An observed address is recorded as a **candidate**, never advertised on first sight | unit |
| P1-2 | Promotion requires **≥2 distinct observing peers** reporting the same address; one peer is never enough | unit |
| P1-3 | Promotion goes through the existing path — `add_external_address` → `ExternalAddrConfirmed` → `NetworkEvent::ExternalAddressConfirmed` → re-announce. No new downstream path | unit + loopback |
| P1-4 | A join ticket minted after promotion carries the confirmed external address | loopback |
| P1-5 | Loopback, link-local, unspecified, and private addresses are never promoted | unit (table) |
| P1-6 | Candidate tracking is bounded; a hostile peer cannot grow memory without limit | unit |
| P1-7 | Candidate and promotion activity is visible in diagnostics | unit |
| P1-8 | No libp2p type crosses into a context crate | code review + existing dependency rules |

### Exclusions

Authoritative reachability (piece 2). Manual override (piece 3). Expiry/pruning of stale confirmed addresses (`ExternalAddrExpired`, `LocalEndpoints` removal) — a real gap, recorded as follow-up, not fixed here. No change to how peers discover *other* peers.

## 2. Entities

No domain entity, aggregate, invariant, or event changes. This piece adds one adapter-local type.

| Name | Kind | Content |
| --- | --- | --- |
| `ExternalAddressLedger` | adapter-local aggregate | `Multiaddr → set of distinct observing `PeerId`s`, plus the set already promoted. Decides promotion. Pure: no swarm, no I/O, no clock. |
| `AddressObservation` | value | `(observer: Libp2pPeerId, address: Multiaddr)` — one peer's claim about us |
| `Promotion` | value | The ledger's verdict for one observation: `Ignored(reason)` \| `Recorded { corroborations }` \| `Promote(Multiaddr)` |
| `CandidateRejection` | value | Why an observation was ignored — `NotGlobal`, `AlreadyPromoted`, `LedgerFull`, `SelfObservation` |

**Invariants (adapter-local, enforced in the ledger):**

1. An address is promoted at most once; a repeat observation of a promoted address is `AlreadyPromoted`, never a second promotion.
2. Corroboration counts **distinct observers**. The same peer reporting the same address twice counts once — otherwise a single hostile peer meets any threshold alone.
3. A non-global address is never recorded and never promoted, whatever the observer count.
4. The ledger holds at most `max_candidate_addresses` addresses and `max_observers_per_address` observers per address; beyond either, further observations are `LedgerFull` and change nothing. *(Clarified during OP-1: the address cap counts **promoted** addresses as well as pending ones — bounding only the pending set would leave the promoted set unbounded against two colluding peers.)*
5. The ledger is a pure decision function of its own state plus the observation — no time, no randomness, no network.

## 3. Approach

**D1 — Consume `SwarmEvent::NewExternalAddrCandidate`; do not read `info.observed_addr` by hand.**
`libp2p-identify` already converts every observed address into a candidate and performs address translation first (`libp2p-identify-0.47.0/src/behaviour.rs:367-383`). Hand-rolling a read of `observed_addr` in the `identify::Event::Received` arm would duplicate that translation, diverge from it on upgrade, and miss candidates from any other behaviour that emits them. Rejected: manual `observed_addr` handling.

**D2 — Corroboration threshold of 2 distinct observers, as a named constant.**
An observed address is a remote peer's claim *about us*. Advertising on one peer's word lets a single hostile peer — identities being free — put an attacker-chosen address into our tickets and DHT records. Two distinct observers reporting an identical address is the smallest rule that is not "trust anyone". Rejected: trusting the first observation (cheap misdirection vector); a high threshold such as 5 (a small network may never have five peers, and the peer would stay unreachable forever — failing the requirement in the common case).

**D3 — The threshold is explicitly an interim, superseded by piece 2.**
AutoNAT v2 confirms an address by having another peer *dial it back*, which is proof rather than corroboration. When piece 2 lands, its verdict is authoritative and this heuristic becomes the fallback for peers with no AutoNAT server available. Recorded here so the constant is not later mistaken for the intended long-term design.

**D4 — Reuse the existing confirmation path unchanged.**
Rejected: emitting a new `NetworkEvent` variant for candidates (nothing in the root would act on it, and the root must contain no policy).

> **CORRECTED 2026-08-05 during OP-1 — the chain this decision assumed does not exist.**
> D4 as first written said `add_external_address` → `SwarmEvent::ExternalAddrConfirmed` → `NetworkEvent::ExternalAddressConfirmed` → re-announce. That is **wrong**, and so were analysis §1 point 4 and the briefing that repeated it. `libp2p-swarm-0.47.1/src/lib.rs:599-605` shows `add_external_address` only broadcasts `FromSwarm::ExternalAddrConfirmed` to the *behaviours* and records the address; the `SwarmEvent` is pushed only at `:1144-1147`, when a **behaviour** emits `ToSwarm::ExternalAddrConfirmed`. Calling it therefore notifies Kademlia, AutoNAT, and the relay client but emits nothing the driver can observe.
> Proved, not argued: the naive implementation was written first and its test failed with `left: [] right: ["/ip4/203.0.113.7/tcp/4001"]` — promotion counted, no event emitted.
> **Resolution, preserving D4's intent** (no new event variant, no parallel pipe): the body of the existing `SwarmEvent::ExternalAddrConfirmed` arm was extracted verbatim into `NetworkDriver::external_address_confirmed`, and promotion calls both — `swarm.add_external_address(addr)` so the behaviours learn it, and `external_address_confirmed(addr)` for the path the root listens on. The existing arm delegates to the same method, so AutoNAT's verdict in piece 2 enters identically. A duplicate confirmation for one address is idempotent downstream: `event_router` gates the re-announce on `LocalEndpoints::record_confirmed`.

**D5 — Reject non-global addresses in the ledger, not at the call site.**
Two peers on the same LAN will both observe each other at `192.168.x.x`, which trivially meets the threshold and would advertise a useless address globally. The check belongs with the decision so it cannot be bypassed by a future second call site. Loopback, link-local, unspecified, private (RFC1918), CGNAT (100.64/10), and IPv6 ULA/link-local are all rejected. Note `/p2p-circuit` addresses are relay addresses, not directly-dialable external ones, and are also rejected here — relay reachability is not what this piece establishes.

**D6 — Diagnostics counters, no UI change.**
The failure mode is silence: the user simply stays unreachable. `CodecDiagnostics` already carries the adapter's counters and is already surfaced in the `d` overlay, so candidates-seen, candidates-recorded, and addresses-promoted go there. A user-facing "you appear reachable at X" line waits for piece 2, when reachability is actually *known* rather than inferred.

## 4. Structure

```text
src/infrastructure/net_libp2p/src/
├── swarm/
│   ├── external_address_ledger.rs        # NEW — the pure decision + its state
│   ├── external_address_ledger_test.rs   # NEW
│   ├── network_driver_test.rs            # NEW (added during OP-1, see below)
│   └── network_driver.rs                 # CHANGED — one new SwarmEvent arm, the ledger,
│                                         #   and the attribution window
├── limits/resource_limits.rs             # CHANGED — two new caps with rationale
├── codec/codec_diagnostics.rs            # CHANGED — three new counters
└── runtime/{mod.rs,network_runtime.rs}   # CHANGED — pub(crate) visibility only, so the
                                          #   driver test assembles the production swarm
```

*(Added during OP-1.)* `network_driver_test.rs` was not foreseen and is load-bearing: safeguard S4 **cannot** be demonstrated on loopback, because every address two loopback peers observe is `127.0.0.1`, which the ledger refuses *before* attribution is ever consulted — so a run with attribution silently broken would look identical to a working one. It supplies real-shaped swarm events to a real swarm and is the only place attribution, exact per-event counter increments, and `Promote → NetworkEvent::ExternalAddressConfirmed` are actually proven. It is also what caught the D4 defect above.

**Dependency direction unchanged.** The ledger is adapter-local and depends only on `libp2p::Multiaddr`/`PeerId` and `std`. Nothing new crosses into a context crate; `Endpoint` conversion continues to happen where it already does, in the existing confirmation arm. No port trait is added, changed, or removed. No context crate is touched by this piece at all.

**Commands vs queries:** not applicable — no application layer is involved. The ledger exposes one mutating decision (`observe`) and pure reads (`is_promoted`, `candidate_count`); they are separate methods and the reads never mutate.

## 5. Operations

Single operation — the change is small, cohesive, and not usefully splittable; splitting the ledger from its wiring would leave a commit with dead code.

**OP-1 — Consume external address candidates with corroboration** *(repo)*

1. `ExternalAddressLedger` with `observe(observer, address) -> Promotion`, the global-address filter, distinct-observer counting, and both bounds. TDD, failing test first.
2. `ResourceLimits` gains `max_candidate_addresses` and `max_observers_per_address`, each with a rationale comment (S6).
3. `CodecDiagnostics` gains `external_candidates_seen`, `external_candidates_recorded`, `external_addresses_promoted`.
4. `NetworkDriver` holds the ledger and handles `SwarmEvent::NewExternalAddrCandidate`. The candidate's observer is **not** carried on that event, so the driver must attribute the observation to the peer whose identify exchange produced it — resolve this at implementation time (see §7/S4); if attribution proves impossible without hand-reading `observed_addr`, **stop and report** rather than silently counting anonymous observations, which would defeat D2 entirely.
5. On `Promotion::Promote`, call `swarm.add_external_address(address)` and let the existing arm do the rest.

**Tests (all required):** first observation records but does not promote; a second distinct observer of the same address promotes exactly once; the same observer twice never promotes; a promoted address re-observed reports `AlreadyPromoted` and does not re-promote; a table of non-global addresses (IPv4 loopback/private/link-local/CGNAT/unspecified, IPv6 loopback/ULA/link-local, `/p2p-circuit`) is never recorded even with many observers; the address-count and observer-count bounds each hold under flood; the ledger is deterministic; diagnostics increment exactly once per event; and a loopback two-swarm test proving the wiring reaches `NetworkEvent::ExternalAddressConfirmed`.

**Verification:** `cargo test -p infra-net-libp2p`, then all four gates: `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`.

## 6. Norms

- `AGENTS.md` — Project Structure: adapters implement ports; no libp2p type crosses into a context crate.
- `AGENTS.md` — Testing: red-green-refactor; co-located `module_test.rs` registered with `#[cfg(test)] mod module_test;`; deterministic tests.
- `AGENTS.md` — Coding Style: one principal implementation per file; hand-written typed errors; no `utils.rs`.
- `AGENTS.md` — Build/Test: the four gates; narrowest check per change, all four before done.
- System canvas `0002` §7/S1 (no operator-run infrastructure), §7/S6 (hostile-input caps), §7/S8 (address disclosure).

## 7. Safeguards

**S1 — Serverless integrity holds.** This piece contacts nothing new. It consumes addresses reported by peers already connected; no resolver, no probe service, no default host. The existing `serverless_integrity_test` guard continues to apply unchanged.

**S2 — One peer's word is never enough.** D2's threshold is the security property of this change, not a tuning knob. Lowering it to 1 reintroduces the misdirection vector and must not be done to "speed up confirmation".

**S3 — Non-global addresses never promote.** Enforced inside the ledger (D5) so it cannot be bypassed by a second call site.

**S4 — Anonymous observations must not count.** If a candidate cannot be attributed to a specific observing peer, it must not be counted toward corroboration — counting unattributed observations makes the threshold meaningless. If the libp2p event shape makes attribution impossible, that is a blocker to surface, not to work around.

**S5 — Bounded state.** Both caps are required, not optional; candidate addresses arrive from untrusted peers (S6 of the system canvas).

**S6 — Wire compatibility untouched.** No envelope, protocol version, ticket format, or persisted file changes shape. Old and new peers interoperate exactly as before; the new one is simply reachable more often. No migration.

**S7 — Privacy disclosure already covers this.** System canvas S8 states that joining announces the peer's addresses. This change makes that statement more accurate rather than adding a new exposure; no new disclosure text is required.

## 8. Agents

| Operation | Agent | Rationale |
| --- | --- | --- |
| OP-1 | `repo` | Entirely `src/infrastructure/net_libp2p/`; persistence/adapter ownership |

`domain-modeler`, `application-handler`, and `api-designer` are **not engaged** — no domain type, no handler, no HTTP surface. `system-architect` review is **not required**: no context boundary, dependency direction, or published contract is touched. `$spdd-sync` runs after OP-1.

## 9. Open confirmations

None blocking. D2's threshold of 2 and the S6 cap values are engineering defaults per the system canvas §9 convention — pinned with rationale comments, not user policy.

**Known follow-up, deliberately not in scope:** confirmed addresses are never expired. `SwarmEvent::ExternalAddrExpired` is unhandled and `LocalEndpoints` is push-only, so an address that stops being valid is advertised indefinitely. Recorded in the analysis §6 and worth its own change.
