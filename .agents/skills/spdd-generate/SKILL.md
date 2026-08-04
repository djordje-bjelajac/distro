---
name: spdd-generate
description: Implement an approved REASONS canvas in this Rust workspace. Use when the user supplies a canvas and asks Codex to execute its ordered operations with specialist agents, TDD, architectural safeguards, and full verification.
---

Require a canvas path. Read the entire canvas, `AGENTS.md`, and affected code before editing.

1. Validate that requirements, operations, safeguards, and agent ownership are coherent. Surface contradictions or missing domain decisions.
2. Track the canvas operations in order. Delegate bounded operations to their named `.codex/agents/` specialists when assigned; parallelize only independent work and wait for all delegated results.
3. For each operation, follow red-green-refactor and keep changes inside its scope.
4. Run the narrowest relevant checks after each operation.
5. At completion run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` when available.
6. Report completed operations, files changed, tests, deviations, and remaining work.

Never claim an unavailable check passed. Do not silently reinterpret the canvas or relax safeguards.
