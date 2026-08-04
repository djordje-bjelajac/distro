# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Current state

The repository contains only agent scaffolding — `AGENTS.md`, `.codex/agents/`, and `.agents/skills/`. There is no Cargo workspace, `src/`, `tests/`, or `docs/` yet. The structure below is the **target** layout to create as code lands, not something already on disk. Verify a path exists before assuming it.

`AGENTS.md` is the authoritative contributor guide. Keep this file consistent with it; when they conflict, `AGENTS.md` wins.

## Commands

```bash
cargo build --workspace                                  # compile every crate
cargo test --workspace                                   # all unit + integration tests
cargo test -p <crate>                                    # one bounded context / infra crate
cargo test -p <crate> <test_name>                        # single test
cargo fmt --all -- --check                               # formatting gate
cargo clippy --workspace --all-targets -- -D warnings    # lint gate (warnings are errors)
```

Run the narrowest relevant check after each change; run all four before declaring work complete. Never report an unavailable check as passing.

## Architecture

Rust Cargo workspace applying DDD, hexagonal architecture, CQRS, and TDD. One bounded context per crate under `src/contexts/<context>/`, with shared contracts in `src/shared_types/`, technical implementations in `src/infrastructure/`, integration tests in `tests/integration/`, and decision records in `docs/`.

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

Naming: ports carry a `Port` suffix, handlers are named by intent (`CreateOrderHandler`), commands are imperative, events are past tense. One principal implementation per file; `lib.rs`/`mod.rs` mostly re-export. No generic `utils.rs`.

## Testing

Red-green-refactor. Co-locate `module_test.rs` beside `module.rs` and register it with `#[cfg(test)] mod module_test;`. Domain and application tests use port fakes and must not touch network, clock, database, or external services; integration tests cover adapter boundaries and must be deterministic. Reproduce a defect with a failing test before fixing it — every bug fix ships with a regression test. Never weaken an assertion to make a test pass.

## SPDD workflow

Non-trivial work flows through the skills in `.agents/skills/`, which are invoked by name (e.g. `$spdd-analysis`):

`spdd-analysis` (requirements → context, ownership, risks; no code) → `spdd-reasons-canvas` (→ a REASONS canvas: Requirements, Entities, Approach, Structure, Operations, Norms, Safeguards, Agents; saved under `docs/specs/` or `spdd/prompt/`) → `spdd-generate` (execute the canvas's ordered operations with TDD and full verification).

`spdd-prompt-update` revises a canvas when scope changes; `spdd-sync` reconciles a canvas against implemented code before a PR. Neither may silently relax requirements, invariants, or safeguards — surface conflicts instead.

An approved canvas is the implementation source of truth, subordinate to `AGENTS.md`. Stop on contradictions or missing domain decisions rather than reinterpreting intent.

## Specialist agents

`.codex/agents/*.toml` define narrow specialists, each owning a layer. The ownership map is the practical expression of the dependency rules above — route work by it, and delegate only bounded, independent operations, reconciling all results before verification (max 6 concurrent threads per session, per `.codex/config.toml`):

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
