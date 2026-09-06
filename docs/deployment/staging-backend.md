# Staging backend deployment

The staging backend uses one Railway project environment, one Supabase staging database, and two Railway services built from `services/backend/Dockerfile`.

## Railway service contract

Configure both services with repository root directory `/services/backend`. Railway detects the `Dockerfile` in that directory and builds its final `runtime` stage.

### API service

- Start command: `api`
- Variables: `APP_ENV=staging`, `DATABASE_URL=<Supabase staging connection string>`, `SUPABASE_URL=https://<staging-project>.supabase.co`, `SUPABASE_PUBLISHABLE_KEY=<staging publishable key>`, `RUST_LOG=info`
- Port: use the `PORT` value injected by Railway; do not configure a fixed port
- Healthcheck path: `/ready`
- Public networking: generate a Railway domain or configure a verified custom domain

### Worker service

- Start command: `worker`
- Variables: `APP_ENV=staging`, `DATABASE_URL=<same Supabase staging connection string>`, `RUST_LOG=info`, `WORKER_HEARTBEAT_INTERVAL_SECONDS=5`
- Public networking: none
- HTTP healthcheck: none; worker health is the persisted PostgreSQL heartbeat read through the API

Start commands remain service settings because one Railway config-as-code file describes one deployment and cannot safely express two different process commands for these two services.

The API rejects plaintext `SUPABASE_URL` values outside `APP_ENV=local`.
`/openapi.json` is authenticated: retrieve it with an enabled user's Supabase
access token in `Authorization: Bearer <token>`. The publishable key is not a
service-role key and no service-role key is required for this deployment.

## First deployment

1. Create or select the dedicated Supabase staging project, obtain its PostgreSQL connection string, configure the GitHub `staging` environment secret `STAGING_DATABASE_URL`, and run the `Staging database` workflow.
2. Create or select the Railway project and its `staging` environment.
3. Create the API and worker services, connect both to this repository, and configure each service using the contract above.
4. Generate public networking only for the API service after it is listening on Railway's injected `PORT`.
5. Create a Railway project token scoped to staging and add these GitHub `staging` environment secrets: `RAILWAY_TOKEN`, `RAILWAY_PROJECT_ID`, `RAILWAY_API_SERVICE_ID`, and `RAILWAY_WORKER_SERVICE_ID`.
6. Add the API HTTPS origin, with no trailing path, as the GitHub `staging` environment variable `STAGING_API_BASE_URL`. Add `STAGING_SUPABASE_URL` and `STAGING_SUPABASE_PUBLISHABLE_KEY` as `staging` environment variables. Add `STAGING_SMOKE_EMAIL` and `STAGING_SMOKE_PASSWORD` as `staging` environment secrets for a dedicated enabled Supabase Auth smoke-test user. The runtime smoke exchanges those credentials for a fresh access token; do not store an access token or a service-role key in GitHub.
7. Add the deployed staging frontend HTTPS origin, with no trailing path, as the GitHub `staging` environment variable `STAGING_WEB_BASE_URL`. The `Staging auth browser E2E` workflow reuses `STAGING_SMOKE_EMAIL` and `STAGING_SMOKE_PASSWORD` for the same enabled dedicated Supabase Auth smoke-test user and tests the deployed frontend through its login UI.
8. Run `Deploy staging backend`, then run `Staging runtime smoke` and `Staging auth browser E2E`.

The deployment workflow uploads `services/backend` as the build root and waits for both Railway deployments to succeed. It assumes the account-specific service commands, variables, healthcheck, and API domain have already been configured; it does not create or guess provider resources.
