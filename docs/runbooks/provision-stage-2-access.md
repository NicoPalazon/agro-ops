# Provision initial Stage 2 staging access

This one-shot backend command creates or verifies the minimum internal authorization path for one existing Supabase staging account. It uses the Supabase `auth.users.id` UUID as the identity, never an email address. It grants only `consola_tecnica:ver` through the organization-scoped `Tecnico` role.

## Prerequisites

- The target database has the complete approved backend migrations, including Slice 2.1 and Slice 2.2.
- You have the staging account's stable Supabase `auth.users.id` UUID.
- You have a PostgreSQL `DATABASE_URL` that can write the staging authorization tables.
- Run the command only after review, commit, and CI approval. This runbook does not authorize production use.

Set the required values in Fish. Replace every placeholder locally; do not commit real UUIDs, database URLs, passwords, tokens, or project URLs.

```fish
set -lx DATABASE_URL '<staging-postgresql-connection-url>'
set -lx AGRO_OPS_PROVISION_SUPABASE_SUBJECT '<stable-supabase-auth-users-id-uuid>'
set -lx AGRO_OPS_PROVISION_ORGANIZATION_NAME '<organization-name>'
set -lx AGRO_OPS_PROVISION_USER_FULL_NAME '<operator-full-name>'

cargo run --manifest-path services/backend/Cargo.toml --bin provision_stage2_access
```

Expected success output contains the internal organization and user UUIDs, `Tecnico`, `consola_tecnica:ver`, and the supplied Supabase subject UUID. It never prints the database URL or credentials.

The command is safe to rerun with the same values: it reuses the matching active organization, identity-linked user, role, and current grants instead of adding duplicates. If a previous `usuarios_roles` or `roles_permisos` episode was closed, it adds a new current episode without changing the historical row.

Stop and investigate manually if it reports an inactive organization, user, role, or canonical permission; duplicate organizations with the same name; a subject linked to another organization; a closed identity link; or a future-dated grant. The command intentionally never reactivates or rewrites historical authorization data.
