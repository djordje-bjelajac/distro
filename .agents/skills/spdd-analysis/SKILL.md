---
name: spdd-analysis
description: Analyze business requirements against this Rust DDD codebase before solution design. Use when a feature request, requirement file, or vague change needs strategic context, bounded-context ownership, domain concepts, risks, and test direction without implementation.
---

Require a requirement description or referenced file. If none is supplied, ask for it and stop.

1. Read `AGENTS.md`, all supplied requirements, and the smallest relevant code/documentation set.
2. Separate repository evidence from assumptions and unresolved questions.
3. Identify the outcome, acceptance criteria, exclusions, owning bounded context, ubiquitous language, invariants, existing capabilities, and affected contracts.
4. Classify likely commands, queries, events, ports, and adapters without prescribing file-by-file edits.
5. Analyze compatibility, migration, security, operational, and testing risks.
6. Produce an analysis suitable as input to `$spdd-reasons-canvas`.

Do not edit code. Focus on what and why; leave detailed implementation sequencing to the canvas.
