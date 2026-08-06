# Repository Guidelines

## Project Structure & Architecture

This Rust Cargo workspace follows DDD, hexagonal architecture, CQRS, and TDD. Model each bounded context as one crate under `src/contexts/<context>/`. Keep shared contracts in `src/shared_types/`, technical implementations in `src/infrastructure/`, integration tests in `tests/integration/`, and decisions in `docs/`.

`src/app/` is the composition root: the single binary crate that wires every context to its adapters and hosts the terminal interface. It belongs to no context, so it sits beside them rather than inside one. It depends on everything and **nothing depends on it**; it holds no domain rule of its own, and it never links test infrastructure such as `src/infrastructure/sim_net/`.

This section describes the layout **on disk today**. `docs/architecture/target-workspace-layout.md` is the approved target — several composition roots under `src/apps/`, with `src/infrastructure/` named by capability rather than by technology — and **every change must move toward it or leave it untouched, never away from it**. Consult it before adding any crate, binary, or top-level directory.

New code lands at its target path. Relocating existing code is separate work: its own commit, a pure move, gates green, with this file and `CLAUDE.md` updated in the same commit — never bundled with a feature. A change that cannot align is a conflict to surface and amend the record for, not to route around; refuse a shared `apps/common` crate in particular, since it would become a composition root everything depends on.

Each context crate should use this layout:

```text
src/contexts/<context>/src/
├── domain/       # Aggregates, value objects, events
├── ports/        # Inbound and outbound traits
├── application/  # CQRS handlers, services, wiring
├── adapters/     # External-system implementations
└── bin/          # Entry points, when required
```

Dependencies point inward: domain imports neither ports nor adapters; application depends on domain and ports; adapters implement ports. Never import another context directly—use published contracts or domain events.

## Build, Test, and Development Commands

- `cargo build --workspace` — compile every workspace crate.
- `cargo test --workspace` — run all unit and integration tests.
- `cargo test -p <crate>` — test one bounded context or infrastructure crate.
- `cargo fmt --all -- --check` — verify formatting.
- `cargo clippy --workspace --all-targets -- -D warnings` — enforce lint-clean code.

## Coding Style & Naming Conventions

Use `rustfmt` and idiomatic Rust casing. Suffix ports with `Port`, name handlers by intent (`CreateOrderHandler`), commands imperatively, and events in past tense. Keep one principal implementation per file; `lib.rs` and `mod.rs` primarily re-export modules. Avoid generic `utils.rs`.

Represent invariants with value objects and explicit domain errors. Keep framework types out of the domain. Separate CQRS command paths, which mutate state, from query paths, which only read it.

## Testing Guidelines

Follow red-green-refactor. Co-locate `module_test.rs` beside `module.rs` and register it with `#[cfg(test)] mod module_test;`. Test domain behavior without infrastructure using port fakes. Add integration tests for adapter boundaries. Every bug fix requires a regression test.

## Codex Workflow

Specialists live in `.codex/agents/`; route work by their declared ownership. They are mirrored into `.claude/agents/*.md` by `scripts/sync-claude-agents.py`, and the `.toml` files remain the source of truth — edit those and regenerate; `scripts/sync-claude-agents.py --check` fails on drift. SPDD workflows live in `.claude/commands/` and are Claude Code only; Codex runs the specialists but has no `$spdd-*` commands. Use them for non-trivial analysis, REASONS canvases, implementation, updates, and synchronization. Delegate only bounded, independent operations and reconcile all results before verification.

## Commit & Pull Request Guidelines

Use focused, imperative commits such as `Add inventory reservation command`. Pull requests must describe behavior, architectural impact, linked issues, and verification. Include evidence for visible changes. Do not merge with failing checks.

## Security & Configuration

Never commit credentials or `.env` files. Provide `.env.example` with placeholder values, validate external input at adapters, and keep secrets and infrastructure configuration outside domain crates.
