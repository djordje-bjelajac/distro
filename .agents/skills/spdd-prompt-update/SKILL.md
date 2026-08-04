---
name: spdd-prompt-update
description: Update an existing REASONS canvas while preserving intent and traceability. Use when requirements, architecture, constraints, operations, or specialist assignments change before or during implementation.
---

Require a canvas path and explicit update instructions. Read the complete canvas, `AGENTS.md`, and relevant repository evidence.

1. Identify which requirements and decisions the requested change affects.
2. Update Requirements, Entities, Approach, Structure, Operations, Norms, Safeguards, and Agents consistently.
3. Preserve unaffected decisions and distinguish new facts from assumptions.
4. Refresh operation dependencies, acceptance checks, test work, and `.codex/agents/` ownership when scope changes.
5. Report changed sections, rationale, affected operations, unresolved decisions, and likely implementation drift.

Do not edit production code or silently discard existing safeguards.
