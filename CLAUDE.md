# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state

The workspace is implemented and green: `Cargo.toml`, `src/shared_types/`, three context crates under `src/contexts/`, three infrastructure crates under `src/infrastructure/`, the `src/app/` composition root and TUI binary, `tests/integration/`, and `docs/specs/`. All four gates pass. The layout below is what is on disk, not a target.

The product is a serverless peer-to-peer text messaging system: every instance is equal, joins without a server, and reaches the network via cached peers → LAN mDNS → a pasted join ticket. `docs/specs/0002-peer-to-peer-communication-canvas.md` is the approved REASONS canvas and the implementation source of truth, with every amendment recorded inline and dated.

`AGENTS.md` is the authoritative contributor guide. Keep this file consistent with it; when they conflict, `AGENTS.md` wins.

## Commands

```bash
cargo build --workspace                                  # compile every crate
cargo test --workspace                                   # all unit + integration tests
cargo test -p <crate>                                    # one bounded context / infra crate
cargo test -p <crate> <test_name>                        # single test
cargo fmt --all -- --check                               # formatting gate
cargo clippy --workspace --all-targets -- -D warnings    # lint gate (warnings are errors)

scripts/sync-claude-agents.py                            # regenerate .claude/agents from .codex/agents
scripts/sync-claude-agents.py --check                    # fail if the two have drifted
```

Run the narrowest relevant check after each change; run all four before declaring work complete. Never report an unavailable check as passing.

## Architecture

Rust Cargo workspace applying DDD, hexagonal architecture, CQRS, and TDD. One bounded context per crate under `src/contexts/<context>/`, with shared contracts in `src/shared_types/`, technical implementations in `src/infrastructure/`, integration tests in `tests/integration/`, and decision records in `docs/`.

**`docs/architecture/target-workspace-layout.md` is the approved target layout, and every change must move toward it or leave it untouched — never away from it.** The layout in this section is what is on disk today; the target is multiple composition roots under `src/apps/` (`tui`, `desktop`, the mobile bridges, `rendezvous`) with `src/infrastructure/` named by capability rather than technology. Read it before adding any crate, binary, or top-level directory. It also records why `apps/rendezvous/` is a reserved slot rather than a decision: it contradicts canvas safeguard S1 as written.

`src/app/` is the composition root — one binary crate wiring every context to its adapters, plus the terminal interface. It belongs to no context, depends on everything, and **nothing depends on it**. It contains no domain rule: if wiring something would require a rule that does not exist, that is a canvas gap to surface, not a decision to take in the root. It must never depend on `src/infrastructure/sim_net/`, which is test infrastructure.

Each context crate:

```text
src/contexts/<context>/src/
├── domain/       # aggregates, value objects, events, invariants, typed errors
├── ports/        # inbound and outbound traits
├── application/  # CQRS handlers, services, wiring
├── adapters/     # external-system implementations
└── bin/          # entry points, when required
```

Rules that are easy to violate and expensive to unwind:

- **Dependencies point inward.** Domain imports neither ports nor adapters. Application depends on domain and ports only. Adapters implement ports. No framework, HTTP, database, or vendor SDK types in the domain.
- **Contexts never import each other.** Communicate only through published contracts in `src/shared_types/` or domain events. A shared repository or table that couples two contexts is a design error, not a shortcut.
- **Commands mutate, queries only read.** Keep the two paths separate all the way through the application layer.
- **A missing port is a blocker, not a detour.** Do not reach past an absent port to a concrete adapter — propose a domain-oriented contract instead.
- **New code lands at its target path, not its legacy one.** A new crate goes where `target-workspace-layout.md` says it belongs; a new composition root goes under `src/apps/` and never becomes a role flag on an existing binary; new infrastructure is named by capability. Relocating *existing* code is separate work — its own commit, a pure move, gates green, with `AGENTS.md` and this file updated in the same commit. Never bundle a move with a feature.
- **A change that cannot align with the target is a conflict to surface, not to route around.** Say which entry it contradicts and why, and let the layout record be amended — the same rule that governs canvas safeguards. A shared `apps/common` crate is the specific shortcut to refuse: it would become a composition root everything depends on.

Naming: ports carry a `Port` suffix, handlers are named by intent (`CreateOrderHandler`), commands are imperative, events are past tense. One principal implementation per file; `lib.rs`/`mod.rs` mostly re-export. No generic `utils.rs`.

## Testing

Red-green-refactor. Co-locate `module_test.rs` beside `module.rs` and register it with `#[cfg(test)] mod module_test;`. Domain and application tests use port fakes and must not touch network, clock, database, or external services; integration tests cover adapter boundaries and must be deterministic. Reproduce a defect with a failing test before fixing it — every bug fix ships with a regression test. Never weaken an assertion to make a test pass.

## SPDD workflow

Non-trivial work flows through the commands in `.claude/commands/`, invoked as `/spdd-analysis` in Claude Code. These are the single definition — the terse `.agents/skills/` copies were removed on 2026-08-06 because two definitions of the same five workflows could resolve differently depending on the tool. **Codex has no SPDD commands as a result**; `.codex/agents/` specialists, including `spdd-executor`, are unaffected.

`spdd-analysis` (requirements → context, ownership, risks; no code) → `spdd-reasons-canvas` (→ a REASONS canvas: Requirements, Entities, Approach, Structure, Operations, Norms, Safeguards, Agents; saved under `docs/specs/` or `spdd/prompt/`) → `spdd-generate` (execute the canvas's ordered operations with TDD and full verification).

`spdd-prompt-update` revises a canvas when scope changes; `spdd-sync` reconciles a canvas against implemented code before a PR. Neither may silently relax requirements, invariants, or safeguards — surface conflicts instead.

An approved canvas is the implementation source of truth, subordinate to `AGENTS.md`. Stop on contradictions or missing domain decisions rather than reinterpreting intent.

## Specialist agents

`.codex/agents/*.toml` define narrow specialists, each owning a layer. The `.toml` files are the source of truth: `.claude/agents/*.md` is generated from them by `scripts/sync-claude-agents.py`, so edit the `.toml` and regenerate rather than editing the Markdown. The ownership map is the practical expression of the dependency rules above — route work by it, and delegate only bounded, independent operations, reconciling all results before verification (max 6 concurrent threads per session, per `.codex/config.toml`):

| Agent | Owns |
| --- | --- |
| `system-architect` | context boundaries, dependency direction, cross-context contracts (read-only; does not edit files) |
| `domain-modeler` | `domain/**`, `ports/**`, `src/shared_types/**` |
| `application-handler` | `application/**` and use-case wiring |
| `api-designer` | HTTP adapters — routes, DTOs, validation, error mapping |
| `repo` | persistence adapters, mappings, migrations, transactions, `src/infrastructure/` DB crates |
| `test-writer` | `*_test.rs`, `tests/integration/**`, fixtures and fakes (never changes production behavior) |
| `spdd-executor` | executing an approved canvas operation whose architecture is already settled |

## Commits and PRs

Focused, imperative commits (`Add inventory reservation command`). PRs describe behavior, architectural impact, linked issues, and verification, with evidence for visible changes. Do not merge with failing checks.

Never commit credentials or `.env` files — provide `.env.example` with placeholders. Validate external input at adapters; keep secrets and infrastructure configuration out of domain crates.
