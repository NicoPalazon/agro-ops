---
name: agro-ops-verify
description: Verify an Agro Ops change using the smallest appropriate set of checks first and the full required gate only when warranted.
---

# Agro Ops Verification

Choose checks based on what changed.

Rust:

- cargo fmt --check
- cargo clippy --all-targets --all-features -- -D warnings
- relevant cargo tests

Frontend:

- lint
- TypeScript typecheck
- relevant frontend tests

Database migration:

- run migrations against clean PostgreSQL/PostGIS
- run relevant DB integration tests

Docker/infrastructure:

- docker compose config
- build or smoke test only when relevant

Cross-stack completed slice:

- run the complete appropriate gate after implementation is finished

Do not repeatedly run expensive full checks after each minor edit.

Never weaken or remove tests to make verification pass.

Report failures precisely and investigate only the relevant failure before
rerunning checks.
