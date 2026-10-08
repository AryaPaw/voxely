---
name: voxely-improve
description: Perform a read-only, evidence-based Voxely improvement audit covering product defects, architecture, tests or UX when the user requests an audit or improvement proposals.
---

# Voxely improvement audit

Accept an optional scope such as UI, architecture, tests or legacy. Without a scope, inspect the current project. A deep audit uses independent read-only tracks and an adversarial review of Critical/High findings. Inherit the current model/settings unless the user or applicable instructions require another selection.

Read root `AGENTS.md`, Git status, relevant implementations, callers, types, tests, configuration and current official documentation. Current code and reproducible evidence outweigh old plans, reports and comments. Preserve unrelated dirty work.

This skill is read-only: don't implement, install packages, start services, commit or mutate external resources unless the user separately authorizes that action. Don't recreate root `TODO.md`.

Inspect applicable areas:

- User-visible defects and inconsistent behavior.
- Dead/obsolete code, competing sources of truth and dependency direction.
- Error propagation, fallbacks, concurrency, lifecycle and data integrity.
- Security, privacy, performance and dependencies.
- False-green tests, coverage gaps and missing native/E2E evidence.
- UX, accessibility, semantic tokens and platform fidelity.
- Observability, delivery and maintainability.

For each finding provide CONFIRMED, HYPOTHESIS or PRODUCT IDEA; Critical/High/Medium/Low severity; exact file/symbol/flow; evidence; impact; remediation boundary; change risk; and the verification needed. Don't describe an idea as a proven defect or propose compatibility for broken private code without a real compatibility requirement. Remove claims that don't survive challenge.

Start with the highest-value actions, then report confirmed problems, supported removal candidates, product ideas and a dependency-ordered recommendation with acceptance criteria. Separate missing live-app, external-access and owner-decision evidence. Use an existing requested plan only when the user asks to save the results there; otherwise return the audit and ask which recommendations to pursue.
