# SPDD Analysis — Reachability Verdict (Piece 2 of 3)

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement:** Consume the AutoNAT v2 verdict, so a peer knows whether it is actually reachable rather than guessing.

**Siblings:** piece 1 (`0003`/`0004`, implemented) consumes identify's address candidates with a corroboration threshold. Piece 3 adds an `--external-address` manual override. This analysis assumes piece 1 is in place.

---

## 1. Repository evidence

Read from the workspace and the vendored `libp2p-autonat-0.15.0` sources.

1. **AutoNAT v2 client already consumes candidates.** `src/v2/client/behaviour.rs:108-113` handles `FromSwarm::NewExternalAddrCandidate` and scores each address. Every candidate identify produces — and, since piece 1, every address the swarm learns of — is already fed to AutoNAT without any wiring from us.
2. **AutoNAT v2 client already confirms.** `src/v2/client/behaviour.rs:202` pushes `ToSwarm::ExternalAddrConfirmed(address)` when a dial-back probe succeeds. A *behaviour* emitting that variant **does** produce `SwarmEvent::ExternalAddrConfirmed` (`libp2p-swarm-0.47.1/src/lib.rs:1144-1147`), which the driver has handled since before piece 1 and which piece 1 routed through `NetworkDriver::external_address_confirmed`.
3. **Therefore the success path is already complete.** A peer that is genuinely reachable, and that can reach an AutoNAT server, already gets its address confirmed and announced today. This must be stated plainly because it means piece 2 is **not** about confirming addresses.
4. **The verdict itself is discarded.** `src/v2/client/behaviour.rs:394-406` defines `Event { tested_addr, bytes_sent, server, result: Result<(), Error> }`, emitted via `ToSwarm::GenerateEvent`. It arrives as `DistroBehaviourEvent::AutonatClient(..)`, and `network_driver.rs` has **no arm for it** — confirmed by zero `autonat` references in that file before this piece.
5. **Consequence — failure is invisible.** A probe that *fails* produces only this event. Nothing else fires. So a peer behind a NAT with no way in has exactly the same observable state as a peer that has simply not been probed yet: silence. The user cannot be told which they are.
6. **The server side is always on.** `distro_behaviour.rs:206` constructs `autonat::v2::server::Behaviour` unconditionally, per canvas AC4 — every instance probes for others. Nothing to change; worth recording that the network can supply verdicts at all because of it.
7. **There is no aggregate status type.** AutoNAT v2, unlike v1's `NatStatus`, exposes no rolled-up reachability enum. Any notion of "am I reachable" must be derived by us from the stream of per-probe results.
8. **`S7` already promises this honesty.** System canvas `0002` §7/S7 requires that the connectivity limit be *stated*, not hidden, and `Usage::DISCLOSURES` tells the user two symmetric-NAT peers may be unable to connect. Today the app can only say that in the abstract; it cannot say "this is happening to you now".

**Correction to an earlier informal claim.** It was said that AutoNAT "probes and nothing acts on the verdict". Half right: the *confirmation* acts (evidence 2), automatically and correctly. The *verdict event* — the only carrier of failure — is what is discarded.

## 2. Outcome

A peer knows, and can say, whether it is reachable from the outside: reachable at a specific address, definitively not reachable, or not yet determined. A user who cannot be messaged learns why from the app rather than by inference, and a peer that knows it is unreachable has the fact available for a future decision to prefer a relay.

## 3. Acceptance criteria (proposed)

| # | Criterion |
| --- | --- |
| P2-1 | A successful probe records the tested address as reachable and keeps the existing confirmation behaviour unchanged. |
| P2-2 | A failed probe is recorded as evidence of unreachability rather than discarded. |
| P2-3 | Reachability is a derived three-state value — `Unknown` (nothing conclusive yet), `Reachable(address)`, `Unreachable` — never a boolean, because "not yet probed" and "probed and refused" are different facts and conflating them would report a lie during startup. |
| P2-4 | A verdict from a **single** server does not settle unreachability; a peer that happens to probe one broken or hostile server must not conclude it is unreachable. Success from one server *is* conclusive — a dial-back that arrived is proof. |
| P2-5 | Reachability is exposed to the composition root through the existing event channel, so the interface can state it. |
| P2-6 | Probe outcomes are visible in diagnostics: how many ran, against which servers, how many succeeded. |
| P2-7 | Reachability moves back to `Reachable` if a later probe succeeds; the state is not a one-way latch. |
| P2-8 | No libp2p type crosses into a context crate. |

### Exclusions

Changing dial or relay *behaviour* based on the verdict — deliberately out of scope. libp2p already prefers a direct address when one is confirmed and falls back to a circuit otherwise; second-guessing it from our side would be a behavioural change with real regression risk, and the value here is in *knowing and saying*. Manual override (piece 3). Any pruning of stale addresses (still the standing follow-up from piece 1 §9).

## 4. Domain analysis

**Owning context: none.** Reachability is a property of this process's network position, not of the messaging, membership, or identity domains. No peer's *identity* or *presence* changes; this is about us, not about them. `membership`'s `Presence` is deliberately unrelated — that is derived evidence about a *remote* peer's liveness, and conflating the two would be a modelling error worth naming so nobody attempts it.

**Adapter-local vocabulary:**

| Term | Meaning |
| --- | --- |
| **Probe** | One AutoNAT dial-back test of one candidate address by one server |
| **Verdict** | The result of a probe — success or a typed failure |
| **Reachability** | The derived three-state answer to "can strangers dial me" |
| **Corroborated unreachability** | Failure reported by ≥2 distinct servers, the bar for concluding `Unreachable` |

**No commands, queries, domain events, or port changes.** One new `NetworkEvent` variant carrying reachability to the root, and diagnostics counters.

## 5. Risks

**Security — asymmetric evidence, deliberately.** Success is self-proving: a dial-back that arrived means the address works, and no attacker benefits from making us believe we are reachable when we are. Failure is not self-proving: a single server that is broken, overloaded, or hostile can report failure for an address that is fine, and a peer that believed it would wrongly conclude it needs a relay. Hence P2-4's asymmetry — one success confirms, one failure does not condemn. This mirrors piece 1's threshold reasoning and should use the same corroboration constant so there is one story about trusting single peers.

**Reporting a false negative is the real user harm.** Telling a reachable user "you are unreachable" sends them to change router settings that were never wrong. Telling an unreachable user nothing is the status quo. So the design should bias toward `Unknown` and only assert `Unreachable` on corroborated evidence.

**Operational.** This is the diagnostic surface for the single most confusing failure in the product ("why can nobody reach my server"). Its own failure mode is silence, so the counters in P2-6 matter as much as the state itself.

**Testing.** The derivation — a stream of `(server, address, result)` into a three-state value — is pure and belongs in its own type with its own tests, exactly as piece 1's ledger did. Piece 1 established that swarm-level attribution needs a driver-level test with supplied events, because loopback cannot produce the relevant conditions; the same applies here and more strongly, since a loopback pair cannot produce a genuine NAT failure at all. **No integration or loopback test can prove real unreachability** — that is the two-machine smoke, still unrun. This limit must be stated in the canvas rather than discovered later.

**Compatibility and migration.** None. No wire format, no persisted file, no protocol version. A new `NetworkEvent` variant is additive within the crate; `app` must handle it, which the compiler will enforce if the enum is matched exhaustively.

**Concurrency.** The verdict arrives on the driver thread and must reach the UI thread through the existing bounded event channel, whose overflow is already counted. No new threading contract.

## 6. Unresolved questions

1. **Should the TUI show reachability, and where?** A status-line element is the obvious home, next to `connected (n)`. *Recommended: yes, and it is the point of the piece — but as a small addition to an existing pane, not a new one.*
2. **How many failing servers before `Unreachable`?** *Recommended: reuse piece 1's corroboration threshold of 2 distinct servers, as one consistent rule about not trusting a single peer.*
3. **Should a confirmed-then-failing address be retracted?** Retraction is the standing pruning follow-up from piece 1 and interacts with `ExternalAddrExpired`. *Recommended: out of scope; reachability may flip to `Unreachable` without retracting the announced address, and that inconsistency should be recorded rather than silently accepted.*

## 7. Specialist routing

`repo` owns the adapter work in `src/infrastructure/net_libp2p/`. `spdd-executor` owns the composition-root and TUI surfacing in `src/app/`, which is wiring and rendering only — no domain rule. No `domain-modeler`, no `application-handler`, no `api-designer`. `system-architect` review not required: no context boundary, dependency direction, or published contract is touched.
