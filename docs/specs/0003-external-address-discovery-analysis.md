# SPDD Analysis — External Address Discovery (Piece 1 of 3)

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement:** A peer cannot learn its own public address, so a port-forwarded home server advertises only LAN addresses and is unreachable from outside. Piece 1 of 3: consume `identify`'s observed address.

**Sibling pieces** (separate analyses, canvases, and commits): piece 2 consumes the AutoNAT v2 verdict; piece 3 adds an `--external-address` manual override. This analysis establishes the shared context the other two will reference.

---

## 1. Repository evidence

Read from the workspace and the vendored crate sources, not inferred:

1. **`identify` already emits candidates.** `libp2p-identify-0.47.0/src/behaviour.rs:370,374,383` pushes `ToSwarm::NewExternalAddrCandidate` for every observed address a remote reports, after attempting address translation. The crate doc at `:93-94` states this is its contract. **The observation machinery is therefore already running today** — nothing needs to be asked for.
2. **The driver never consumes it.** `network_driver.rs` handles `SwarmEvent::{NewListenAddr, ListenerError, ListenerClosed, ExternalAddrConfirmed, ConnectionEstablished, ConnectionClosed, OutgoingConnectionError, Behaviour}`. There is **no `NewExternalAddrCandidate` arm**, so every candidate identify produces is discarded by the catch-all.
3. **The confirmation path already exists and works.** `network_driver.rs:426` handles `SwarmEvent::ExternalAddrConfirmed`, records the address in `self.announced`, and emits `NetworkEvent::ExternalAddressConfirmed`. The composition root consumes that at `event_router.rs:97-101`, records it in `LocalEndpoints`, and re-announces the full set. **Everything downstream of confirmation is built and tested.**
4. **Nothing ever confirms.** `Swarm::add_external_address` (`libp2p-swarm-0.47.1/src/lib.rs:599`) is the only call that produces `ExternalAddrConfirmed`, and the sole call site is `network_driver.rs:342`, inside the `announce` path — which re-adds addresses the peer *already* knew. No new address can enter that way.
5. **AutoNAT is constructed but unconsumed.** `distro_behaviour.rs:202-206` builds both v2 client and server; `network_driver.rs` contains zero `autonat` references. In libp2p's design AutoNAT is the component that promotes a candidate to confirmed — that is piece 2, and its absence is why candidates would otherwise never be promoted.
6. **`info.observed_addr` is separately unused.** The app's own `identify::Event::Received` handler (`network_driver.rs:592`) takes `info.listen_addrs` for peer discovery and ignores `observed_addr`. This is *not* the defect — the candidate path above is the supported mechanism — but it confirms no second route exists.
7. **`ExternalAddrExpired` is also unhandled**, and `LocalEndpoints` (`app/src/composition/local_endpoints.rs`) is push-only: `record_listening`, `record_confirmed`, `all`, with no removal. Stale addresses accumulate. Known, out of scope here, noted in §6.
8. **Constraints that bind this change.** `AGENTS.md`: adapters implement ports, no libp2p type crosses into a context crate. Canvas §7/S1: no operator-run infrastructure in any code path. Canvas §7/S6: hostile input is capped at this boundary.

**Correction to an earlier informal claim:** it was previously stated that `observed_addr` is "dropped on the floor". More precisely: identify converts it into a candidate automatically, and it is *the candidate* that the driver discards. The fix is narrower and better-supported than a hand-rolled reading of `observed_addr` would be.

## 2. Outcome

A peer that is genuinely reachable from outside its NAT — because a port is forwarded, or it holds a public address, or IPv6 reaches it — learns that address from the peers that can see it, and advertises it. Its join tickets, its DHT records, and its announcements then carry an address a stranger can actually dial.

## 3. Acceptance criteria (proposed)

| # | Criterion |
| --- | --- |
| P1-1 | An address observed by a remote peer is recorded as a **candidate**, not immediately advertised. |
| P1-2 | A candidate is promoted to a confirmed external address only after **at least two distinct peers** independently report the same address. One peer's word is never sufficient. |
| P1-3 | On promotion the address flows through the existing confirmation path — `add_external_address` → `ExternalAddrConfirmed` → `NetworkEvent::ExternalAddressConfirmed` → re-announce — with no new downstream path invented. |
| P1-4 | A join ticket minted after promotion carries the confirmed external address alongside the local ones. |
| P1-5 | Loopback, link-local, and unspecified addresses are never promoted; a peer on the same LAN observing `192.168.x.x` must not cause that to be advertised as external. |
| P1-6 | The number of distinct candidates tracked is bounded; a hostile peer cannot grow memory by reporting endless addresses. |
| P1-7 | Candidate and confirmation activity is visible in diagnostics — a user asking "why am I unreachable" gets an answer, not silence. |
| P1-8 | No libp2p type crosses into a context crate; the mapping stops at the adapter. |

### Exclusions (this piece)

Authoritative reachability determination (piece 2). Manual override (piece 3). Pruning stale/expired addresses (`ExternalAddrExpired`, `LocalEndpoints` removal) — a real gap, but a separate change with its own risk. Any change to how peers *discover other peers*.

## 4. Domain analysis

**Owning context: none.** This is entirely an infrastructure concern inside `infra-net-libp2p`. `Endpoint` (a `membership` value object) is the only domain type involved and it already exists with the right shape — an opaque address string plus a `Reachability` class. No aggregate, invariant, or port changes. This is the correct outcome: "how does this process discover its own public address" is a transport detail, and the domain is right not to know about it.

**Ubiquitous language addition (adapter-local):**

| Term | Meaning |
| --- | --- |
| **Observed address** | An address a remote peer reports seeing us arrive from. Hearsay until corroborated. |
| **Candidate** | An observed address recorded but not advertised. |
| **Confirmed external address** | A candidate corroborated well enough to advertise. |
| **Corroboration threshold** | The number of distinct observers required for promotion. |

**No commands, queries, or domain events.** The unit of change is the driver's swarm-event loop plus a small candidate ledger.

**Ports and adapters:** no port changes. `PeerDiscoveryPort::announce` is already the outbound path and already receives whatever `LocalEndpoints` holds. One existing `NetworkEvent` variant (`ExternalAddressConfirmed`) is reused as-is.

## 5. Risks

**Security — the reason for the threshold.** An observed address is a *claim by a remote peer about us*. A single malicious peer that reports an attacker-chosen address could get us to advertise it, feeding bogus addresses into the DHT and tickets: a cheap eclipse/misdirection vector, and free to attempt since identities cost nothing (canvas §6, Sybil). Requiring corroboration from ≥2 distinct peers, and rejecting non-global addresses outright, makes the attack require multiple colluding observers reporting an identical address. Piece 2 replaces this heuristic with AutoNAT's authoritative dial-back verdict, which is strictly stronger; the threshold is the honest interim.

**Privacy.** Advertising a confirmed public address is exactly what canvas S8 already discloses ("joining announces this peer's network addresses"). This change makes the disclosure *more* true rather than introducing a new exposure. No new disclosure text is needed, but the claim should be re-read for accuracy.

**Resource.** Candidate tracking is attacker-influenced input and needs a bound (S6). Without one, a peer that reports a fresh address per identify exchange grows the ledger without limit.

**Correctness — NAT address translation.** identify may translate an observed address using local listen information before emitting it (`behaviour.rs:367-376`), so candidates are not always literally what the remote saw. Behaviour is inherited from the crate and correct; worth stating so a future reader does not "fix" it.

**Compatibility.** Wire format untouched — this changes only which addresses a peer advertises, and the `Envelope`/`ProtocolVersion` contract is unaffected. No migration; nothing persisted changes shape. An older peer and a newer peer interoperate exactly as before, the newer one simply being reachable more often.

**Testing.** The unit is a pure decision — given a sequence of (observer, address) observations, which addresses are promoted — and should be extracted so it is testable without a swarm: the threshold, the non-global rejection, and the bound all belong in a small ledger type with its own tests. The swarm-level wiring gets one loopback test. `infra-sim-net` has no concept of a public address, so this cannot be covered at the multi-peer integration level; that is a real limit and should be stated rather than papered over. Final proof is the two-machine smoke, still unrun.

**Operational.** This is the change that decides whether a port-forwarded server is reachable. Its failure mode is silent by nature — the user simply stays unreachable — so P1-7's diagnostics are not decoration; they are how anyone will ever debug it.

## 6. Unresolved questions

1. **Corroboration threshold value.** 2 is the smallest number that is not "trust anyone". Higher is safer but slower to confirm, and on a small network there may only ever be two peers. *Recommended: 2, as a named constant with rationale, superseded by AutoNAT in piece 2.*
2. **Does a confirmed address ever expire?** Handling `ExternalAddrExpired` and giving `LocalEndpoints` a removal path is the natural companion, but it is a distinct change with its own failure modes. *Recommended: out of scope here; record as follow-up.*
3. **Should the UI surface reachability?** A "you appear reachable at X" line is valuable, but reachability is only truly known after piece 2. *Recommended: diagnostics counters here, UI status in piece 2.*

## 7. Specialist routing

`repo` owns this end to end — it is `src/infrastructure/net_libp2p/` only. No `domain-modeler` involvement (no domain change), no `application-handler` (no handler change), no `api-designer` (no HTTP). `system-architect` review is not required: no context boundary, dependency direction, or published contract is touched.
