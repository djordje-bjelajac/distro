# Target workspace layout

**Status:** approved target, not yet on disk. Recorded 2026-08-06.
**Standing:** binding. Every change must move toward this layout or leave it untouched — never away from it. `AGENTS.md` and `CLAUDE.md` both carry the rule; a change that cannot align is a conflict to surface and amend this record for, not to route around.
**Purpose:** the shape this workspace evolves toward as it grows a second, third and fourth composition root. Nothing here changes behaviour; it constrains where new code lands and records what each move costs.

`AGENTS.md` still describes the layout **on disk today** and remains authoritative for current work. This file describes the destination. When a migration step lands, `AGENTS.md` and `CLAUDE.md` are updated in the same commit — never after.

---

## 1. The target

```text
src/
├── contexts/                 # Shared domain and application logic
│   ├── identity/
│   ├── membership/
│   └── messaging/
├── shared_types/             # Protocol DTOs, IDs, wire contracts
├── infrastructure/
│   ├── networking/
│   ├── persistence/
│   └── cryptography/
├── apps/
│   ├── tui/                  # Terminal composition root
│   ├── desktop/              # Windows/macOS/Linux composition root
│   ├── ios_bridge/           # Rust library exported to Swift
│   ├── android_bridge/       # Rust library exported to Kotlin
│   └── rendezvous/           # Community rendezvous server
ios/                          # Xcode project and SwiftUI shell
android/                      # Gradle project and Android UI
```

## 2. What changes, and what does not

**Unchanged.** The three context crates and `shared_types` are already at their target paths. The internal shape of a context — `domain/`, `ports/`, `application/`, `adapters/` — does not change, nor does any dependency rule in `AGENTS.md`. Every constraint that holds today holds after the migration; this is a re-homing, not a re-architecture.

**Renames.**

| Today | Target | Crate name |
| --- | --- | --- |
| `src/infrastructure/net_libp2p/` | `src/infrastructure/networking/` | `infra-net-libp2p` → `infra-networking` |
| `src/infrastructure/store_fs/` | `src/infrastructure/persistence/` | `infra-store-fs` → `infra-persistence` |
| `src/app/` | `src/apps/tui/` | `app` → `app-tui` |

The renames drop the *technology* from the path. That is the point — `networking` can host a second transport without the directory lying — but it also removes a signal that was doing real work: `net_libp2p` announced its containment rule (canvas D2: no `libp2p` type crosses the adapter boundary) in its own name. That rule must survive the rename as a documented, tested constraint rather than a naming convention.

**New homes.**

- `infrastructure/cryptography/` — today crypto is split between `store_fs/src/crypto/` (at-rest file encryption) and the `ed25519-dalek` use inside the identity domain. A shared crate is justified once a second consumer needs the same primitive; until then, moving code here is churn. **This directory stays empty until a real second consumer exists.**
- `apps/desktop/`, `apps/ios_bridge/`, `apps/android_bridge/` — additional composition roots. The bridges are libraries (`cdylib`/`staticlib`), not binaries; being a library does not make them anything other than composition roots, and the rules in §3 apply to them in full.
- `apps/rendezvous/` — see §5. It is the one entry in this layout that is not merely a move.

**Unaddressed by the target, and deliberately kept.**

- `src/infrastructure/sim_net/` — deterministic test fabric (virtual clock, seeded PRNG, in-memory stores). It has no home in the list above and does not belong under `networking/`: it spans networking *and* persistence, and it is test infrastructure that no shipped binary may link. It moves to `src/infrastructure/simulation/` and keeps its existing prohibition — **no `apps/*` crate may depend on it**, which is currently enforced for `src/app` and must be re-enforced per app crate.
- `tests/integration/` — unchanged, stays at the workspace root.

## 3. Rules the multi-root shape adds

One composition root needed one rule: *nothing depends on `app`*. Five roots need three.

1. **No crate depends on any `apps/*` crate.** The existing rule, generalised.
2. **`apps/*` crates never depend on each other.** `rendezvous` must not import `tui`; `desktop` must not import `tui`. There is no such thing as a "mostly a composition root".
3. **No shared `apps/common` crate.** This is the failure mode the shape invites: four roots wire the same contexts, the wiring looks duplicated, and a helper crate appears to hold it. That crate would become a composition root that everything depends on — precisely the thing rule 1 exists to prevent. If two roots genuinely need the same wiring, the duplication is a signal that the logic belongs in a context's application layer, where it can be tested against ports. Duplicated wiring in two roots is cheaper than a shared root, and the pain of copying it is the design pressure working correctly.

Each root wires a different subset. `tui` and `desktop` wire all three contexts; the bridges wire all three but expose a foreign-function surface instead of a UI; `rendezvous` wires `membership` alone and links no messaging UI at all. The subsets differing is what makes them separate roots rather than one root with flags.

## 4. Sequencing

Each step lands as its own commit with all four gates green. None of these are urgent; each should be pulled by a feature that needs it, not pushed as tidying.

1. **`src/app/` → `src/apps/tui/`.** Mechanical, but touches every path reference, the workspace manifest, and both guide files. Do it first and alone — while there is exactly one root, so a mistake is visible immediately.
2. **The two infrastructure renames.** Independent of step 1 and of each other. Pure `use`-path and manifest churn.
3. **`sim_net` → `simulation/`,** with its per-app-crate prohibition re-established as a test.
4. **`apps/rendezvous/`,** pulled by the rendezvous work — and blocked on §5.
5. **`apps/desktop/`, the bridges, `ios/`, `android/`,** each pulled by an actual platform target. The bridges bring a new class of problem (FFI, panics across the boundary, no `main`) that this record does not attempt to settle in advance.
6. **`infrastructure/cryptography/`,** last and only on demand.

Steps 1–3 are refactors with no behavioural change: they must not be combined with a feature commit, and their diffs should be reviewable as pure moves.

## 5. `apps/rendezvous/` contradicts the system canvas as written

Recorded here because the layout cannot be approved while pretending otherwise.

Canvas `0002` rules this out in three places:

- **§1 exclusions** — "any operator-run infrastructure" is out of scope.
- **D1** — rejects "public rendezvous" *by name* as "operator-run infrastructure [that] violates 'no servers' — the requirement the user ranked above convenience", and rejects "iroh-style relay+rendezvous infrastructure (servers)".
- **S1** — "no default relay/rendezvous/STUN endpoints", listed under safeguards as non-negotiable.

S1 is not only prose. `src/infrastructure/net_libp2p/src/serverless_integrity_test.rs` fails the build if a forbidden dependency enters the graph, and `src/app/src/cli/` has tests guarding specifically against a `--bootstrap` or `--relay-address` shaped option appearing. A rendezvous server is a deliberate reversal of a reasoned decision, defended by tests that will go red on purpose.

**Why the reversal is being considered.** D1's cost was stated honestly and has come due: when every cached peer is offline *and* its address has since changed, and no peer is on the LAN, the ladder has no rung left. The remaining path is a manual ticket — which requires a live peer to mint it. Two peers that are both cold and both mobile cannot reach each other at all. D1 predicted a one-time cost; the observed cost is recurring and, in the worst case, unrecoverable.

**What must be preserved.** S1's purpose is not the absence of hosts; it is that **no one can switch the network off**. That property survives a rendezvous server only if all of the following hold, and the amendment is worth nothing unless they are testable:

- No rendezvous address is compiled in, defaulted, or shipped. A fresh install with no configuration reaches the network exactly as it does today, or not at all.
- The existing three rungs keep working, unchanged, with zero rendezvous configured — and are still tested that way.
- A rendezvous is never trusted for identity. It relays what peers signed; it cannot mint, vouch for, or alter an identity. A hostile rendezvous must be able to withhold and to lie, and no lie it tells may be believed.
- The privacy cost is stated, not buried: a peer that uses a rendezvous discloses its address set and its online times to whoever runs that host. Under D1 there was nobody to disclose them to.

**This file does not make that amendment.** Changing a safeguard requires `$spdd-prompt-update` against canvas `0002`, and the identity-validation design the rendezvous needs is `$spdd-analysis` work. Until both land, `apps/rendezvous/` is a reserved slot in this layout and nothing more — no crate, no manifest entry.

## 6. Consequences to accept

- **The renames are wide and shallow.** Nearly every file in the workspace gains a changed `use` line. Cheap to do, noisy in history, and worth doing before there are five roots rather than after.
- **`src/apps/` reads as an application layer to anyone arriving from the DDD literature.** It is not; it is a set of composition roots. The name is worth its familiarity, but the guide files must say so explicitly or someone will put a use case in it.
- **Five roots multiply the wiring surface.** Every new port added to a context is now wired in up to five places, and a root that forgets is a runtime hole rather than a compile error. `desktop` and the bridges should not be created speculatively — each one is a standing maintenance cost from the day it exists.
