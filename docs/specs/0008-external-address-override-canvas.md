# REASONS Canvas — External Address Override (Piece 3 of 3)

**Status:** **IMPLEMENTED 2026-08-05.** OP-1, OP-2, OP-3 complete; all eight acceptance criteria proven; workspace 1407 tests, all four gates exit 0, binary smoke verified including the private-address refusal. Amendments from the reconciliation are marked inline. **S5 still stands: nothing here proves an asserted address actually works from outside** — that is the operator's claim, and the two-machine smoke of system canvas OP-13 remains unrun. Subordinate to `AGENTS.md` and to the system canvas `0002`, which it extends without amending.
**Input:** `docs/specs/0007-external-address-override-analysis.md`.
**Scope:** `src/app/` and `src/infrastructure/net_libp2p/`. No domain, application, or context crate change.

---

## 1. Requirements

### Outcome

A user who has forwarded a port can tell their instance the address the world reaches it at, and that address is advertised — in announcements, DHT records, and join tickets — without waiting for another peer to observe or probe it. A home server can be the **first** reachable peer on a network rather than needing one to already exist.

### Acceptance criteria

| # | Criterion | Verified by |
| --- | --- | --- |
| P3-1 | `--external-address <MULTIADDR>` is accepted, repeatable, and each value is advertised | unit + driver |
| P3-2 | A supplied address reaches the same confirmation path pieces 1 and 2 use; no parallel pipe | driver |
| P3-3 | A malformed multiaddress is refused at startup, naming the offending value; it never silently does nothing | unit |
| P3-4 | A supplied address appears in a join ticket minted afterwards | loopback |
| P3-5 | Help text states plainly this is **this peer's own address, not a host to contact** | unit (string assertion) |
| P3-6 | The no-role-flag guard is updated deliberately and still refuses every name it refuses today | unit |
| P3-7 | An override does not disable observation (piece 1) or probing (piece 2); a later AutoNAT verdict still applies | unit + driver |
| P3-8 | A non-global address is rejected, with a message noting mDNS already covers the local network | unit (table) |

### Exclusions

Persisting the override (launch-time fact; a profile setting is a different feature with its own migration story). Automatic port mapping — UPnP/NAT-PMP is a genuinely useful separate change and emphatically not this one. Retraction or expiry of a supplied address (standing follow-up since `0004` §9).

## 2. Entities

No domain entity, aggregate, invariant, port, or event changes. This piece adds two configuration fields and one startup behaviour.

| Name | Kind | Content |
| --- | --- | --- |
| `LaunchOptions::external_addresses` | config field | `Vec<String>`, repeatable, empty by default |
| `NetworkConfig::external_addresses` | config field | The validated set the runtime advertises at startup |

**Invariants:**

1. An override is **asserted**, never proven. It is advertised on the operator's word alone — which is why 2 and 3 exist.
2. A non-global address is rejected at parse time, for the same reason piece 1 refuses to promote one: advertising a private address globally is never useful.
3. An override never suppresses evidence. Observation and probing continue; a later AutoNAT failure still reports `Unreachable` (P3-7). Assertion is the weakest of the three sources, not the strongest.
4. A malformed value is a startup refusal, never a silent no-op — this is the option a user reaches for when nothing else worked, so it must not fail quietly.
5. The value is **this peer's own address** and is never dialled. Nothing may turn it into a peer to contact.

## 3. Approach

**D1 — Advertise immediately, and let evidence contradict it.**
The entire point is to serve the peer with nobody to ask; waiting for AutoNAT to agree would reproduce the deadlock this piece exists to break. A wrong assertion is corrected by piece 2 reporting `Unreachable`, which is exactly the honesty that piece was built for. Rejected: advertising only after AutoNAT confirms (defeats the purpose); advertising and suppressing probes (would make the app confidently wrong, the one outcome piece 2's asymmetric evidence was designed to avoid).

**D2 — Reuse the existing confirmation path.**
`swarm.add_external_address(addr)` plus `NetworkDriver::external_address_confirmed(addr)`, exactly the pairing piece 1 established after discovering `add_external_address` alone emits no `SwarmEvent`. Announcements, DHT records, and join tickets then follow with no new code. Rejected: a separate advertise path (three sources of advertised addresses would then need three sets of tests, and would drift).

**D3 — Reject non-global addresses, reusing piece 1's filter.**
Piece 1 already owns a tested predicate covering IPv4 loopback/private/link-local/CGNAT/unspecified/multicast/broadcast, IPv6 loopback/ULA/link-local, IPv4-mapped, and `/p2p-circuit`. Reuse it rather than writing a second one that will diverge. The refusal message should note mDNS already covers the local network, so a user attempting `192.168.x.x` learns they do not need the flag at all. Rejected: warning instead of rejecting (a warning in a TUI app that immediately clears the screen is not seen).

**D4 — The help text carries the S1 distinction explicitly, and a test pins it.**
This option is *shaped* like the bootstrap-host option the project forbids: a multiaddress on the command line. The distinction is total — it is our own address, only ever advertised, never dialled — but it is invisible to someone skimming `--help`. `--listen` already faces this and solves it with an explicit sentence pinned by a test; this option does the same. Rejected: relying on the option name to convey it.

**D5 — Update the two CLI guard tests deliberately, and strengthen one.**
`there_is_no_role_flag_and_no_way_to_add_one_quietly` and `the_whole_option_set_is_six_options_and_two_requests` will both fail on this change. They exist to force exactly this moment of thought. The first must keep refusing all 16 names and additionally refuse the near-misses this option creates (`--external-peer`, `--external-host`, `--advertise-peer`) — names that would be a bootstrap list wearing this option's clothes. The second becomes seven options; it stays an exhaustive struct literal so an eighth still fails to compile. Rejected: deleting either test, or adding `--external-address` to an allow-list without widening the refusals.

**D6 — Diagnostics show that an override is in effect.**
"I set the flag" and "the flag took effect" must be distinguishable without a debugger, since this option is the last resort when nothing else worked.

> *(Refined during OP-3.)* "In effect" is the **intersection** of supplied and confirmed, not a raw confirmation tally. Observation (piece 1) and probing (piece 2) confirm through the same `ExternalAddressConfirmed` event, so a raw count would read `1` on a launch that passed no flag — precisely the confusion D6 exists to remove. Supplied values are normalised through `EndpointMapping::parse` before comparison, because libp2p re-renders addresses (`/ip6/2001:db8:0:0:0:0:0:1/…` returns as `/ip6/2001:db8::1/…`) and comparing spellings verbatim would report a working override as broken. Deliberately **no** counter was added to `CodecDiagnostics`: an assertion is not an observation, and mixing them beside `external_candidates_seen` would blur the same distinction.

## 4. Structure

```text
src/app/src/
├── cli/launch_options.rs        # CHANGED — external_addresses field + parsing
├── cli/launch_options_test.rs   # CHANGED — the two guards (D5) + new cases
├── cli/usage.rs                 # CHANGED — help text (D4)
├── cli/usage_test.rs            # CHANGED — pin the S1 sentence
├── composition/node.rs          # CHANGED — pass the validated set into NetworkConfig
└── composition/diagnostics.rs   # CHANGED — record that an override is in effect

src/infrastructure/net_libp2p/src/
├── runtime/network_config.rs    # CHANGED — external_addresses field
├── runtime/network_runtime.rs   # CHANGED — apply at startup via the shared path
├── swarm/network_driver.rs      # CHANGED (not foreseen) — `assert_external_address`; the
│                                #   add_external_address + external_address_confirmed pairing
│                                #   only exists where the swarm does
└── swarm/external_address_ledger.rs  # CHANGED — global-address predicate now pub(crate) (D3);
                                      #   the 16-row NON_GLOBAL table moved here so both call
                                      #   sites are asserted against one list

src/app/src/main.rs               # CHANGED (not foreseen) — the options→config mapping was
                                  #   inline in `run()`, which nothing can call; extracted as
                                  #   pure `network_of()` so §5's "the option reaches
                                  #   NetworkConfig" is testable at all
```

*(Reconciled 2026-08-05.)* Two test files were added that §4 did not list — `app/src/main_test.rs` and `app/src/composition/diagnostics_test.rs` (the latter type had never had one, and its two lists are the first thing in `Diagnostics` that decides anything).

**Dependency direction unchanged.** No port trait added or changed. No context crate touched. `app` depends on everything; nothing depends on `app`. The global-address predicate becomes `pub(crate)` within `infra-net-libp2p` — it does not leave the adapter.

**Commands vs queries:** not applicable; no application layer involved.

## 5. Operations

**OP-1 — Accept and validate the option** *(spdd-executor)*
`LaunchOptions::external_addresses`, repeatable parsing, startup refusal on a malformed value naming it (P3-3), help text with the S1 distinction (P3-5/D4), and both guard tests updated per D5. Validation of *multiaddress syntax* happens here; the *global-address* check belongs with the predicate in OP-2 and is asserted end-to-end in OP-3.

*Tests:* a single value parses; repeated values accumulate in order; a malformed value is refused and the message names it; the help text contains the "own address, not a host to contact" distinction; the 16 forbidden names plus the three new near-misses all stay unknown; the option set is exhaustively seven options and two requests.

*Verification:* `cargo test -p app`, then all four gates.

**OP-2 — Advertise it through the shared path** *(repo)*
`NetworkConfig::external_addresses`; at startup, for each value, reject non-global using piece 1's predicate (made `pub(crate)`, D3) and otherwise call `swarm.add_external_address` **and** `external_address_confirmed`, the pairing piece 1 established. Must not disable observation or probing (invariant 3).

*Tests:* a supplied global address reaches `NetworkEvent::ExternalAddressConfirmed`; a supplied non-global address does not and is refused with the mDNS note; several addresses all advertise; an override does not stop the candidate ledger recording observations, and does not stop AutoNAT probes being recorded (P3-7 — assert both explicitly, since this is the one place the three pieces could contradict each other); a loopback test proving a supplied address appears in a minted join ticket (P3-4).

*Verification:* `cargo test -p infra-net-libp2p`, then all four gates.

**OP-3 — Wire and surface** *(spdd-executor)*
Pass the validated set from `LaunchOptions` through `Node::start` into `NetworkConfig`; record in `Diagnostics` that an override is in effect and show it in the `d` overlay (D6). Update `src/app/README.md` and the root `README.md` where they describe reaching a peer from outside.

*Tests:* the option reaches `NetworkConfig`; diagnostics report an override when one is supplied and not when none is; the binary smoke (`--help`, `--print-identity`) still exits 0.

*Verification:* `cargo test -p app`, all four gates, and the binary smoke.

## 6. Norms

- `AGENTS.md` — adapters implement ports; no libp2p type in a context crate; `app` holds no domain rule.
- `AGENTS.md` — Testing: red-green-refactor; co-located `module_test.rs`; deterministic tests; never weaken an assertion.
- `AGENTS.md` — Coding Style: one principal implementation per file; hand-written typed errors.
- `AGENTS.md` — the four gates.
- System canvas `0002` §7/S1 (no operator-run infrastructure), AC4 (no role flags), §7/S7 (state the connectivity limit).

## 7. Safeguards

**S1 — This option must never become a bootstrap list.** The value is this peer's **own** address, advertised only, **never dialled**. Nothing may pass it to `dial`, a peer cache, a ticket's issuer field, or Kademlia as a peer address. This is the safeguard most at risk from a later well-meaning edit — the option already looks like the thing the project forbids — which is why D4's help text and D5's widened guard are non-negotiable parts of the change, not documentation.

**S2 — An assertion never outranks evidence.** Invariant 3. Supplying an override must not suppress observation or probing, and must not force a `Reachable` display. A user who asserts a wrong address must still be told it does not work; that honesty is the whole point of piece 2 and this piece must not undo it.

**S3 — Non-global addresses are refused, not warned.** A warning in an app that immediately clears the screen for a TUI is a warning nobody reads.

**S4 — Loud failure.** This is the last-resort option; a malformed value refuses at startup naming the value, and diagnostics distinguish "supplied" from "in effect". *(Implemented with a split, recorded during OP-2: the **malformed** refusal names the offending value and happens at the CLI before anything is built; the **non-global** refusal names the class rather than the value, because `NetworkStartError` is `Copy` and `app`'s `StartError` derives `Copy` and delegates `Display`. Naming the non-global value would break both; if it is wanted, that is a canvas decision, not an adapter edit.)*

**S7 — The globality check lives where it cannot be bypassed.** It is applied inside the one driver method that reaches the swarm, not at the startup parse — the same reasoning as `0004`'s S3. Consequence, accepted: the refusal happens after the transports are built, so its runtime-level test needs a swarm and skips (loudly, via `required_network`) on a machine that cannot build one. The exhaustive 16-row table is asserted at driver level, which needs that swarm anyway.

**S5 — What this cannot prove.** That the asserted address actually works from outside is the operator's claim, testable only by the unrun two-machine smoke of system canvas OP-13. Nothing in this piece may be presented as verifying reachability — that is piece 2's job, and its verdict remains authoritative.

**S6 — Wire compatibility untouched.** No envelope, protocol version, ticket format, or persisted file changes shape. Purely additive at the command line; every existing invocation behaves identically. No migration.

## 8. Agents

| Operation | Agent | Rationale |
| --- | --- | --- |
| OP-1 | `spdd-executor` | `src/app/` CLI, help text, guard tests |
| OP-2 | `repo` | `src/infrastructure/net_libp2p/` adapter behaviour |
| OP-3 | `spdd-executor` | `src/app/` wiring, diagnostics, docs |

Sequenced, not parallel: OP-2 needs OP-1's field shape, OP-3 needs both. `domain-modeler`, `application-handler`, `api-designer` not engaged. `system-architect` review not required — no context boundary, dependency direction, or published contract is touched. `$spdd-sync` runs after OP-3.

## 9. Open confirmations

None blocking. Repeatability (matching `--listen`) and rejection-over-warning are settled in §3; both follow existing precedent in this codebase rather than introducing policy.

**Standing follow-up, still not in scope:** no advertised address is ever retracted or expired, so a supplied address outlives its usefulness until the process ends. First raised in `0004` §9, unchanged by this piece, and now with a third source of advertised addresses feeding it.
