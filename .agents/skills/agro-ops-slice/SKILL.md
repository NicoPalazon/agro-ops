---
name: agro-ops-slice
description: Implement one bounded Agro Ops development slice while preserving the repository architecture and minimizing unnecessary repository exploration.
---

# Agro Ops Slice

Use the repository AGENTS.md as the authoritative baseline.

For the requested slice:

1. Identify the smallest relevant set of files.
2. Inspect only those files and directly related code.
3. Do not perform broad repository analysis unless the task cannot be completed otherwise.
4. Preserve the agreed architecture.
5. Do not introduce speculative abstractions or future-stage functionality.
6. Implement the requested behavior completely.
7. Add or update focused tests for the behavior and invariants changed.
8. Run focused checks while fixing implementation issues.
9. When the slice is complete, run the appropriate final verification.
10. Return a concise report:

- summary
- files changed
- tests added/changed
- verification performed
- unresolved issues, if any

Do not commit or push unless explicitly requested.
