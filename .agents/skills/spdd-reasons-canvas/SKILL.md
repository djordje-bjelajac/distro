---
name: spdd-reasons-canvas
description: Convert requirements or an SPDD analysis into an implementation-ready REASONS canvas for this Rust DDD repository. Use when a non-trivial feature needs explicit requirements, domain entities, architecture, ordered operations, norms, safeguards, agent ownership, and tests before coding.
---

Require business context, requirement files, or an SPDD analysis. Read `AGENTS.md` and relevant repository evidence.

Create a Markdown canvas containing:

1. **Requirements** — outcome, acceptance criteria, exclusions.
2. **Entities** — aggregates, value objects, events, invariants, relationships.
3. **Approach** — decisions, rationale, and rejected alternatives.
4. **Structure** — crates, modules, ports, adapters, dependency direction.
5. **Operations** — ordered, independently verifiable implementation steps.
6. **Norms** — applicable repository rules by reference.
7. **Safeguards** — compatibility, security, migration, and non-negotiable constraints.
8. **Agents** — assign each operation to the narrowest `.codex/agents/` specialist.

Keep commands separate from queries, preserve context autonomy, and include tests and verification with every operation. Mark assumptions and decisions requiring user confirmation. Save under `docs/specs/` or `spdd/prompt/` when requested.
