# REASONS Canvas — Reachability Verdict (Piece 2 of 3)

**Status:** **IMPLEMENTED 2026-08-05.** OP-1 and OP-2 complete; all eight acceptance criteria proven by 31 new tests (13 ledger, 7 driver, 12 app); workspace 1373 tests, all four gates exit 0, binary smoke clean. Amendments from the reconciliation are marked inline. **S4 still stands unchanged: nothing here proves real unreachability** — the two-machine smoke of system canvas OP-13 remains unrun. Subordinate to `AGENTS.md` and to the system canvas `0002`, which it extends without amending.
**Input:** `docs/specs/0005-reachability-verdict-analysis.md`.
**Scope:** `src/infrastructure/net_libp2p/` and `src/app/`. No domain, application, or context crate change.

---

## 1. Requirements

### Outcome

A peer knows, and can say, whether strangers can dial it: reachable at a specific address, definitively not, or not yet determined. The user who cannot be messaged learns why from the app instead of inferring it.

### Acceptance criteria

| # | Criterion | Verified by |
| --- | --- | --- |
| P2-1 | A successful probe records the address as reachable; existing confirmation behaviour is unchanged | unit + driver |
| P2-2 | A failed probe is recorded as evidence, not discarded | unit + driver |
| P2-3 | Reachability is three-state — `Unknown` \| `Reachable(Endpoint)` \| `Unreachable` — never a boolean | unit (type) |
| P2-4 | One success confirms reachability; one failure never concludes unreachability (≥2 distinct servers required) | unit (asymmetry table) |
| P2-5 | Reachability reaches the composition root over the existing event channel | driver + app unit |
| P2-6 | Probe counts — run, succeeded, failed — are in diagnostics | unit |
| P2-7 | A later success returns the state to `Reachable`; it is not a one-way latch | unit |
| P2-8 | No libp2p type crosses into a context crate | code review |

### Exclusions

Changing dial or relay *behaviour* from the verdict — libp2p already prefers a confirmed direct address and falls back to a circuit; overriding that is real regression risk for no gain here. Manual override (piece 3). Address pruning/retraction (standing follow-up since piece 1).

## 2. Entities

No domain entity or invariant changes. Two adapter-local types plus one event variant.

| Name | Kind | Content |
| --- | --- | --- |
| `ReachabilityLedger` | adapter-local aggregate | Per-address failure evidence keyed by distinct server `PeerId`, plus the currently reachable address. Pure: no swarm, no I/O, no clock. |
| `Reachability` | value | `Unknown` \| `Reachable(Endpoint)` \| `Unreachable`. *(Implemented with `#[default] Unknown` so the root holds it without an `Option`.)* **Name collision, recorded during OP-1:** `membership::domain::Reachability` already exists and means something different — `Direct`/`Relayed`, a property of one *address*. This one is a property of *this peer's network position*. Both were kept; any file holding both aliases one. Renaming either needs a `$spdd-prompt-update`. |
| `ProbeOutcome` | value | The ledger's verdict for one probe: `Unchanged` \| `Changed(Reachability)` |
| `ProbeResult` | value | *(Added during OP-1, not foreseen.)* `Succeeded` \| `Failed`. Forced: `autonat::v2::client::Error` has a `pub(crate)` field and no constructor, so `Result<(), Error>` is unconstructible in a test and must not enter a pure ledger. The driver translates at the boundary. |
| `NetworkEvent::ReachabilityChanged(Reachability)` | event | Carries the derived state to the composition root |

**Invariants (adapter-local):**

1. **Evidence is asymmetric.** One successful probe sets `Reachable`. Failures never set `Unreachable` below the corroboration threshold.
2. **`Unreachable` requires failures from ≥2 distinct servers.** The same server failing repeatedly counts once — otherwise one broken or hostile server condemns a peer that is fine.
3. **`Unknown` is the honest default** and must be distinguishable from `Unreachable` at all times. Startup is `Unknown`, not `Unreachable`.
4. **Not a latch.** A success after failures returns the state to `Reachable`, and its failure evidence is cleared.
5. The ledger emits `Changed` only on an actual transition — a repeated identical verdict is `Unchanged`, so the root is not woken for nothing.
6. Failure evidence is bounded; probe results arrive from untrusted servers (system canvas S6).

## 3. Approach

**D1 — Consume the client `Event`; do not touch the confirmation path.**
`libp2p-autonat-0.15.0/src/v2/client/behaviour.rs:202` already emits `ToSwarm::ExternalAddrConfirmed` on success, and that already reaches `NetworkDriver::external_address_confirmed` via the arm piece 1 refactored. **The success path is complete and must not be re-implemented.** This piece adds an arm for `DistroBehaviourEvent::AutonatClient(Event { tested_addr, server, result, .. })` and reads the verdict — nothing more. Rejected: deriving reachability from `ExternalAddrConfirmed` alone (it can never carry failure, which is the entire point).

**D2 — Asymmetric evidence: one success confirms, two failures condemn.**
A dial-back that arrived is proof; no attacker gains by convincing us we are reachable. A failure is hearsay from one server that may be broken, overloaded, or hostile. Telling a reachable user "you are unreachable" sends them to change router settings that were never wrong — a worse outcome than saying nothing. Reuses piece 1's corroboration threshold so there is one story about not trusting a single peer. Rejected: symmetric treatment (produces confident false negatives); trusting any single failure (same, more often).

**D3 — Three states, never a boolean.**
`Unknown` and `Unreachable` are different facts. A boolean forces startup to claim one of them, and it would claim the alarming one for every peer during the first seconds of every launch. Rejected: `bool`; `Option<bool>` (same information, no names, and reads as a nullable flag at every call site).

**D4 — Report only; change no dial behaviour.**
The verdict informs the user. libp2p's own address selection already does the right thing with a confirmed address. Acting on our derived state would duplicate that logic with worse information. Rejected: forcing relay-only mode on `Unreachable` (would strand a peer whose probe was wrong).

**D5 — Reachability is not `Presence`.**
`membership::Presence` is derived evidence about a *remote* peer's liveness. Reachability is a property of *this* process's network position. They are structurally similar and semantically unrelated; conflating them — or putting reachability in a context crate — would be a modelling error. It stays in the adapter, and travels to the root as a `NetworkEvent` like every other network fact.

**D6 — Surface it in the status line, next to `connected (n)`.**
That is where a user already looks to answer "is this thing working". `Unknown` renders as nothing at all rather than as a spinner or a warning, because during normal startup it is neither interesting nor actionable.

## 4. Structure

```text
src/infrastructure/net_libp2p/src/
├── swarm/
│   ├── reachability_ledger.rs        # NEW — pure derivation + evidence
│   ├── reachability_ledger_test.rs   # NEW
│   ├── network_event.rs              # CHANGED — one new variant
│   ├── network_driver.rs             # CHANGED — one AutonatClient arm, holds the ledger
│   └── network_driver_test.rs        # CHANGED — supplied-event coverage
├── limits/resource_limits.rs         # CHANGED — one bound, with rationale
└── codec/codec_diagnostics.rs        # CHANGED — three counters

src/app/src/
├── runtime/event_router.rs           # CHANGED — route the new variant
├── composition/diagnostics.rs        # CHANGED — hold last known reachability
└── tui/status_line.rs                # CHANGED — render it
```

**Dependency direction unchanged.** `Reachability` converts to nothing domain-side; it reaches `app` as an adapter type, exactly as `NetworkEvent` variants already do. No port trait added or changed. No context crate touched. `app` depends on everything and nothing depends on `app`, as before.

**Commands vs queries:** not applicable — no application layer involved. The ledger has one mutating method (`record`) and pure reads; they are separate and the reads never mutate.

## 5. Operations

**OP-1 — Derive and report reachability** *(repo)*
`ReachabilityLedger` with `record(server, address, result) -> ProbeOutcome` implementing invariants 1-6; `Reachability` and `ProbeOutcome`; `NetworkEvent::ReachabilityChanged`; the `AutonatClient` arm in the driver; one bound in `ResourceLimits` with rationale; three counters in `CodecDiagnostics`. TDD, failing test first, co-located tests.

*Tests:* one success → `Reachable`; one failure → still `Unknown` (**the asymmetry, and the most important test here**); two failures from distinct servers → `Unreachable`; one server failing twice → still `Unknown`; success after `Unreachable` → `Reachable` with evidence cleared (P2-7); repeated identical verdict → `Unchanged`; startup is `Unknown` and is never equal to `Unreachable`; the failure-evidence bound holds under flood; determinism; counters increment exactly once per probe; plus driver-level coverage with supplied `AutonatClient` events proving the arm reaches `NetworkEvent::ReachabilityChanged` (loopback cannot produce a NAT failure — see §7/S4).

*Verification:* `cargo test -p infra-net-libp2p`, then all four gates.

**OP-2 — Surface it to the user** *(spdd-executor)*
Route `NetworkEvent::ReachabilityChanged` in `event_router`, hold the latest value in `Diagnostics`, render it in the status line beside `connected (n)`. `Unknown` renders as nothing. Wording must be honest and actionable: `Reachable` names the address; `Unreachable` says a relay will be needed and does not imply the user broke something.

*Tests:* the router maps the variant to the stored state; the status line renders each of the three states, and renders nothing for `Unknown`; the unreachable wording contains no instruction the user cannot act on.

*Verification:* `cargo test -p app`, then all four gates plus the binary smoke (`--help`, `--print-identity`).

## 6. Norms

- `AGENTS.md` — adapters implement ports; no libp2p type in a context crate; `app` contains no domain rule.
- `AGENTS.md` — Testing: red-green-refactor; co-located `module_test.rs`; deterministic tests.
- `AGENTS.md` — Coding Style: one principal implementation per file; hand-written typed errors.
- `AGENTS.md` — the four gates.
- System canvas `0002` §7/S1 (no operator-run infrastructure), §7/S6 (hostile-input caps), §7/S7 (state the connectivity limit rather than hiding it).

## 7. Safeguards

**S1 — Serverless integrity holds.** AutoNAT servers are ordinary peers already connected; the server side is unconditionally on (canvas AC4). Nothing new is contacted, no default host is introduced, and the `serverless_integrity_test` guard applies unchanged.

**S2 — Never report a false negative.** D2's asymmetry is the user-facing safety property of this change. Concluding `Unreachable` from one server's failure would send users to fix routers that were never broken. The threshold is not a tuning knob.

**S3 — `Unknown` must stay distinguishable from `Unreachable`.** Collapsing them — in the type, the router, or the renderer — reintroduces exactly the false negative S2 forbids, during every startup.

**S4 — Known limit, stated not hidden.** **No automated test can prove real unreachability.** Loopback peers cannot produce a NAT failure, and `infra-sim-net` has no concept of a public address. Driver-level tests with supplied events prove the *logic*; the real path is the two-machine smoke of system canvas OP-13, **which has not been run**. This must not be presented as more proven than it is.

**S5 — Do not change dial behaviour.** D4. The verdict reports; libp2p decides. Making relay selection depend on our derived state is out of scope and carries regression risk.

**S6 — Bounded evidence.** Probe results come from untrusted servers; the failure ledger needs a cap like every other attacker-influenced structure at this boundary. *(Implemented as one bound, `max_failing_addresses = 16`, where piece 1 needed two: an address is condemned at the threshold and then accepts no further evidence, so servers-per-address is already capped by `CORROBORATION_THRESHOLD` itself.)*

**S8 — Two refinements made in the S2 direction, recorded during OP-1.** Both go beyond the literal wording of invariants 1-4 and both were chosen to make a false negative *less* likely, never more:
- **A success clears all failure evidence, not just that address's.** Proof that one address works is proof strangers can dial this peer; leaving stale failures would let the very next failed probe flip a just-proven peer to `Unreachable`.
- **A condemned address only displaces proof about itself.** A multi-homed peer whose IPv4 path is blocked but IPv6 path works stays `Reachable`. `Unreachable` requires corroborated failure *and* no address currently proven reachable.

**S7 — Wire compatibility untouched.** No envelope, version, ticket, or persisted file changes. No migration.

## 8. Agents

| Operation | Agent | Rationale |
| --- | --- | --- |
| OP-1 | `repo` | `src/infrastructure/net_libp2p/` — adapter ownership |
| OP-2 | `spdd-executor` | `src/app/` composition root and TUI; wiring and rendering only |

`domain-modeler`, `application-handler`, `api-designer` not engaged — no domain type, handler, or HTTP surface. `system-architect` review not required: no context boundary, dependency direction, or published contract is touched. `$spdd-sync` runs after OP-2.

## 9. Open confirmations

None blocking. The corroboration threshold (2, reused from piece 1) and the evidence bound are engineering defaults per system canvas §9 — pinned with rationale comments, not user policy.

**Recorded during OP-2, not decided:** the status line's green/amber colour keys off `is_isolated()` only, so an `Unreachable` verdict does not repaint it. Making a *report-only* verdict change the alarm colour is a rendering rule this canvas does not state, and inventing one in the composition root is precisely what OP-2 forbids. Left as-is; worth a decision if a follow-up wants it.

**Standing follow-up, still not in scope:** a confirmed address is never retracted, so a peer may report `Unreachable` while still advertising a previously confirmed address. Inconsistent, recorded deliberately rather than silently accepted; it belongs with the `ExternalAddrExpired`/pruning change first raised in `0004` §9.
