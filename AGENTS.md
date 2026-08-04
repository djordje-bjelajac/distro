# Repository Guidelines

## Project Structure & Architecture

This Rust Cargo workspace follows DDD, hexagonal architecture, CQRS, and TDD. Model each bounded context as one crate under `src/contexts/<context>/`. Keep shared contracts in `src/shared_types/`, technical implementations in `src/infrastructure/`, integration tests in `tests/integration/`, and decisions in `docs/`.

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

Specialists live in `.codex/agents/`; route work by their declared ownership. Repository SPDD skills live in `.agents/skills/`. Use them for non-trivial analysis, REASONS canvases, implementation, updates, and synchronization. Delegate only bounded, independent operations and reconcile all results before verification.

## Commit & Pull Request Guidelines

Use focused, imperative commits such as `Add inventory reservation command`. Pull requests must describe behavior, architectural impact, linked issues, and verification. Include evidence for visible changes. Do not merge with failing checks.

## Security & Configuration

Never commit credentials or `.env` files. Provide `.env.example` with placeholder values, validate external input at adapters, and keep secrets and infrastructure configuration outside domain crates.
