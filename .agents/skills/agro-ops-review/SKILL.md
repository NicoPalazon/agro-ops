---
name: agro-ops-review
description: Review an Agro Ops diff for correctness, architecture, persistence, concurrency, idempotency, security and test coverage.
---

# Agro Ops Review

Review the current change, not the entire repository.

Start from the diff and inspect only directly relevant surrounding code.

Check:

1. Architecture
   - modular monolith boundaries
   - API/worker/jobs separation
   - external systems behind adapters
   - no unapproved architecture changes

2. Correctness
   - business and infrastructure invariants
   - error handling
   - edge cases

3. Database
   - transaction boundaries
   - constraints
   - PostgreSQL/PostGIS semantics
   - migration safety
   - Decimal/NUMERIC where required

4. Reliability
   - idempotency
   - retries
   - concurrency
   - reversibility/history

5. Security
   - authorization boundaries
   - secret exposure
   - unsafe internal endpoints

6. Tests
   - changed behavior is demonstrated
   - relevant failure cases exist
   - existing tests were not weakened

Return findings ordered by severity.

Do not rewrite unrelated code.
Do not commit or push.
