#!/usr/bin/env bash

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
backend_dir="${repo_root}/services/backend"
image_tag="${BACKEND_DOCKER_SMOKE_IMAGE:-agro-ops-backend:smoke}"
api_port="${BACKEND_DOCKER_SMOKE_PORT:-8091}"
postgres_user="agro_ops_ci"
postgres_password="agro_ops_ci_smoke_password"
postgres_database="agro_ops_ci_smoke"
container_database_url="postgres://${postgres_user}:${postgres_password}@postgres:5432/${postgres_database}"
run_id="${RANDOM}${RANDOM}$$"
network_name="agro-ops-smoke-${run_id}"
postgres_container="agro-ops-smoke-postgres-${run_id}"
api_container="agro-ops-smoke-api-${run_id}"
worker_container="agro-ops-smoke-worker-${run_id}"
supabase_auth_container="agro-ops-smoke-supabase-auth-${run_id}"
postgres_volume="agro-ops-smoke-postgres-data-${run_id}"
temporary_dir="$(mktemp -d)"
containers=()
smoke_access_token="ci-smoke-access-token"
smoke_publishable_key="sb_publishable_ci_smoke"
smoke_supabase_subject="11111111-2222-4333-8444-555555555555"
supabase_auth_url="http://supabase-auth:8080"

cleanup() {
    local container attempt

    for container in "${containers[@]}"; do
        docker rm --force --volumes "${container}" >/dev/null 2>&1 || true
    done

    docker volume rm --force "${postgres_volume}" >/dev/null 2>&1 || true

    for attempt in {1..5}; do
        if docker network rm "${network_name}" >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    rm -rf "${temporary_dir}"
}
trap cleanup EXIT

fail() {
    echo "backend Docker smoke failed: $*" >&2
    exit 1
}

wait_for_postgres() {
    local attempt

    for attempt in {1..30}; do
        if docker exec "${postgres_container}" \
            pg_isready --username "${postgres_user}" --dbname "${postgres_database}" \
            >/dev/null 2>&1; then
            return
        fi
        sleep 1
    done

    fail "PostgreSQL did not become ready within 30 seconds"
}

wait_for_host_postgres() {
    local attempt

    for attempt in {1..30}; do
        if DATABASE_URL="${migration_database_url}" \
            sqlx migrate info --source "${backend_dir}/migrations" >/dev/null 2>&1; then
            return
        fi
        sleep 1
    done

    fail "PostgreSQL host connection did not become ready within 30 seconds"
}

wait_for_container_running() {
    local container="$1"
    local attempt running

    for attempt in {1..30}; do
        running="$(docker inspect --format '{{.State.Running}}' "${container}" 2>/dev/null || true)"
        if [[ "${running}" == "true" ]]; then
            return
        fi
        sleep 1
    done

    fail "${container} did not remain running within 30 seconds"
}

wait_for_http_200() {
    local path="$1"
    local response_file="${temporary_dir}/$(tr '/' '_' <<<"${path}").json"
    local attempt status

    for attempt in {1..30}; do
        status="$(curl --silent --show-error --output "${response_file}" --write-out '%{http_code}' \
            "http://127.0.0.1:${api_port}${path}" 2>/dev/null || true)"
        if [[ "${status}" == "200" ]]; then
            return
        fi
        sleep 1
    done

    fail "${path} did not return HTTP 200 within 30 seconds"
}

wait_for_healthy_worker() {
    local response_file="${temporary_dir}/worker-status.json"
    local attempt status diagnostic="<empty response>"

    for attempt in {1..30}; do
        : >"${response_file}"
        status="$(curl --silent --show-error --output "${response_file}" --write-out '%{http_code}' \
            --header "Authorization: Bearer ${smoke_access_token}" \
            "http://127.0.0.1:${api_port}/internal/worker/status" 2>/dev/null || true)"
        if [[ "${status}" == "200" ]] \
            && grep --fixed-strings --quiet '"status":"healthy"' "${response_file}"; then
            return
        fi
        sleep 1
    done

    if [[ -s "${response_file}" ]]; then
        diagnostic="$(head -c 512 "${response_file}" | tr '\n' ' ')"
    fi
    fail "worker status did not become healthy within 30 seconds; last HTTP status ${status:-curl-error}; response: ${diagnostic}"
}

wait_for_stale_worker() {
    local response_file="${temporary_dir}/worker-status-after-stop.json"
    local attempt status

    for attempt in {1..30}; do
        status="$(curl --silent --show-error --output "${response_file}" --write-out '%{http_code}' \
            --header "Authorization: Bearer ${smoke_access_token}" \
            "http://127.0.0.1:${api_port}/internal/worker/status" 2>/dev/null || true)"
        if [[ "${status}" == "200" ]] \
            && grep --fixed-strings --quiet '"status":"stale"' "${response_file}"; then
            return
        fi
        sleep 1
    done

    fail "stopped worker did not become stale within 30 seconds"
}

assert_worker_status_requires_authentication() {
    local status

    status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
        "http://127.0.0.1:${api_port}/internal/worker/status" 2>/dev/null || true)"
    [[ "${status}" == "401" ]] \
        || fail "worker status without a bearer token returned HTTP ${status}, expected 401"
}

assert_mock_received_authenticated_request() {
    docker logs "${supabase_auth_container}" 2>&1 \
        | grep --fixed-strings --quiet 'verified /auth/v1/user request' \
        || fail "API did not verify the synthetic bearer token through mock Supabase Auth"
}

worker_heartbeat_snapshot() {
    docker exec "${postgres_container}" \
        psql --username "${postgres_user}" --dbname "${postgres_database}" \
        --tuples-only --no-align \
        --command "SELECT count(*), EXTRACT(EPOCH FROM max(last_seen_at)) FROM service_heartbeats WHERE service_name = 'worker';"
}

assert_worker_heartbeat_row() {
    local expected_timestamp_comparison="$1"
    local snapshot row_count last_seen_epoch timestamp_updated

    snapshot="$(worker_heartbeat_snapshot)"
    IFS='|' read -r row_count last_seen_epoch <<<"${snapshot}"
    [[ "${row_count}" == "1" ]] || fail "expected exactly one persisted worker heartbeat row, found ${row_count}"

    if [[ -n "${expected_timestamp_comparison}" ]]; then
        timestamp_updated="$(docker exec "${postgres_container}" \
            psql --username "${postgres_user}" --dbname "${postgres_database}" \
            --tuples-only --no-align \
            --command "SELECT (EXTRACT(EPOCH FROM last_seen_at) > ${expected_timestamp_comparison})::int FROM service_heartbeats WHERE service_name = 'worker';")"
        [[ "${timestamp_updated}" == "1" ]] \
            || fail "restarted worker did not update the existing heartbeat row"
    fi

    printf '%s\n' "${last_seen_epoch}"
}

start_worker() {
    docker run --detach --name "${worker_container}" --network "${network_name}" \
        --env APP_ENV=staging \
        --env "DATABASE_URL=${container_database_url}" \
        --env WORKER_HEARTBEAT_INTERVAL_SECONDS=1 \
        "${image_tag}" worker >/dev/null
    containers+=("${worker_container}")
}

assert_container_exited_zero() {
    local container="$1"
    local service="$2"
    local exit_code

    exit_code="$(docker inspect --format '{{.State.ExitCode}}' "${container}")"
    if [[ "${exit_code}" != "0" ]]; then
        fail "${service} exited with code ${exit_code} after Docker SIGTERM"
    fi
}

assert_log_has_no_secret() {
    local log_file="$1"

    if grep --fixed-strings --quiet -- "${container_database_url}" "${log_file}" \
        || grep --fixed-strings --quiet -- "${postgres_password}" "${log_file}"; then
        fail "startup logs exposed the CI database URL or password"
    fi
}

assert_startup_fails() {
    local check_name="$1"
    shift
    local log_file="${temporary_dir}/${check_name}.log"

    if docker run --rm "$@" >"${log_file}" 2>&1; then
        fail "${check_name} unexpectedly exited successfully"
    fi

    assert_log_has_no_secret "${log_file}"
}

seed_smoke_authorization() {
    local permission_count

    permission_count="$(docker exec "${postgres_container}" \
        psql --username "${postgres_user}" --dbname "${postgres_database}" \
        --tuples-only --no-align \
        --command "SELECT count(*) FROM public.permisos WHERE codigo = 'consola_tecnica:ver';")"
    [[ "${permission_count}" == "1" ]] \
        || fail "expected exactly one canonical consola_tecnica:ver permission after migrations"

    docker exec "${postgres_container}" \
        psql --set ON_ERROR_STOP=1 --username "${postgres_user}" --dbname "${postgres_database}" \
        --command "
            WITH organizacion AS (
                INSERT INTO public.organizaciones (nombre)
                VALUES ('Docker smoke authorization')
                RETURNING id
            ),
            usuario AS (
                INSERT INTO public.usuarios (organizacion_id, nombre_completo)
                SELECT id, 'Docker smoke technical operator'
                FROM organizacion
                RETURNING id
            ),
            identidad AS (
                INSERT INTO public.identidades_autenticacion_externas
                    (usuario_id, proveedor, sujeto_proveedor)
                SELECT id, 'supabase', '${smoke_supabase_subject}'::uuid
                FROM usuario
            ),
            rol AS (
                INSERT INTO public.roles (organizacion_id, nombre)
                SELECT organizacion.id, 'Tecnico smoke'
                FROM organizacion
                RETURNING id
            ),
            usuario_rol AS (
                INSERT INTO public.usuarios_roles (usuario_id, rol_id)
                SELECT usuario.id, rol.id
                FROM usuario
                CROSS JOIN rol
            )
            INSERT INTO public.roles_permisos (rol_id, permiso_id)
            SELECT rol.id, permiso.id
            FROM rol
            CROSS JOIN public.permisos AS permiso
            WHERE permiso.codigo = 'consola_tecnica:ver';
        " >/dev/null
}

docker network create "${network_name}" >/dev/null

docker volume create "${postgres_volume}" >/dev/null

docker run --detach --name "${postgres_container}" --network "${network_name}" --network-alias postgres \
    --mount type=volume,source="${postgres_volume}",target=/var/lib/postgresql/data \
    --env "POSTGRES_USER=${postgres_user}" \
    --env "POSTGRES_PASSWORD=${postgres_password}" \
    --env "POSTGRES_DB=${postgres_database}" \
    --publish "127.0.0.1::5432" \
    postgis/postgis:17-3.5 >/dev/null
containers+=("${postgres_container}")

wait_for_postgres

postgres_port="$(docker inspect --format '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "${postgres_container}")"
migration_database_url="postgres://${postgres_user}:${postgres_password}@127.0.0.1:${postgres_port}/${postgres_database}?sslmode=disable"
wait_for_host_postgres
DATABASE_URL="${migration_database_url}" sqlx migrate run --source "${backend_dir}/migrations"
seed_smoke_authorization

docker run --detach --name "${supabase_auth_container}" --network "${network_name}" --network-alias supabase-auth \
    --env "SMOKE_ACCESS_TOKEN=${smoke_access_token}" \
    --env "SMOKE_PUBLISHABLE_KEY=${smoke_publishable_key}" \
    --env "SMOKE_SUPABASE_SUBJECT=${smoke_supabase_subject}" \
    python:3.13-alpine \
    python -c '
import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer

class SupabaseAuthHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/auth/v1/user":
            self.send_error(404)
            return
        expected_authorization = "Bearer " + os.environ["SMOKE_ACCESS_TOKEN"]
        if self.headers.get("Authorization") != expected_authorization or self.headers.get("apikey") != os.environ["SMOKE_PUBLISHABLE_KEY"]:
            self.send_error(401)
            return
        body = json.dumps({"id": os.environ["SMOKE_SUPABASE_SUBJECT"]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        print("verified /auth/v1/user request", flush=True)

    def log_message(self, format, *args):
        pass

HTTPServer(("0.0.0.0", 8080), SupabaseAuthHandler).serve_forever()
' >/dev/null
containers+=("${supabase_auth_container}")
wait_for_container_running "${supabase_auth_container}"

docker run --detach --name "${api_container}" --network "${network_name}" \
    --publish "127.0.0.1:${api_port}:${api_port}" \
    --env APP_ENV=local \
    --env "DATABASE_URL=${container_database_url}" \
    --env "PORT=${api_port}" \
    --env "SUPABASE_URL=${supabase_auth_url}" \
    --env "SUPABASE_PUBLISHABLE_KEY=${smoke_publishable_key}" \
    "${image_tag}" api >/dev/null
containers+=("${api_container}")

start_worker

wait_for_http_200 /health
wait_for_http_200 /ready
wait_for_http_200 /version
assert_worker_status_requires_authentication
wait_for_healthy_worker
assert_mock_received_authenticated_request
initial_heartbeat_epoch="$(assert_worker_heartbeat_row '')"

docker stop --time 10 "${worker_container}" >/dev/null
assert_container_exited_zero "${worker_container}" worker
wait_for_stale_worker

docker rm "${worker_container}" >/dev/null
start_worker
wait_for_healthy_worker
assert_worker_heartbeat_row "${initial_heartbeat_epoch}" >/dev/null

docker stop --time 10 "${api_container}" >/dev/null
assert_container_exited_zero "${api_container}" API

docker logs "${api_container}" >"${temporary_dir}/api.log" 2>&1
docker logs "${worker_container}" >"${temporary_dir}/worker.log" 2>&1
assert_log_has_no_secret "${temporary_dir}/api.log"
assert_log_has_no_secret "${temporary_dir}/worker.log"

assert_startup_fails api-missing-database-url "${image_tag}" api
assert_startup_fails api-invalid-app-env \
    --env "DATABASE_URL=${container_database_url}" \
    --env APP_ENV=invalid \
    "${image_tag}" api
assert_startup_fails api-invalid-port \
    --env "DATABASE_URL=${container_database_url}" \
    --env PORT=invalid \
    "${image_tag}" api
assert_startup_fails worker-missing-database-url "${image_tag}" worker

echo "backend Docker smoke passed"
