---
name: spdd-sync
description: Reconcile a REASONS canvas with implemented Rust code and tests. Use at commit, before a pull request, after refactoring, or when suspected canvas-to-code drift must be identified and resolved without weakening requirements.
---

Require a canvas path. Read the complete canvas, `AGENTS.md`, relevant diff or commits, affected code, and tests.

1. Compare every requirement and operation with concrete implementation evidence.
2. Update implementation details, structure, operation status, verification evidence, and agent ownership where the code legitimately refined the design.
3. Do not silently relax requirements, invariants, or safeguards; surface conflicts needing product or architecture decisions.
4. Preserve the distinction between intended and currently implemented behavior.
5. Run relevant verification when needed to establish status.
6. Report synchronized sections, unresolved drift, check results, and whether the canvas can be marked implemented.
