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
postgres_volume="agro-ops-smoke-postgres-data-${run_id}"
temporary_dir="$(mktemp -d)"
containers=()

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
    local attempt status

    for attempt in {1..30}; do
        status="$(curl --silent --show-error --output "${response_file}" --write-out '%{http_code}' \
            "http://127.0.0.1:${api_port}/internal/worker/status" 2>/dev/null || true)"
        if [[ "${status}" == "200" ]] \
            && grep --fixed-strings --quiet '"status":"healthy"' "${response_file}"; then
            return
        fi
        sleep 1
    done

    fail "worker status did not become healthy within 30 seconds"
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
migration_database_url="postgres://${postgres_user}:${postgres_password}@127.0.0.1:${postgres_port}/${postgres_database}"
DATABASE_URL="${migration_database_url}" sqlx migrate run --source "${backend_dir}/migrations"

docker run --detach --name "${api_container}" --network "${network_name}" \
    --publish "127.0.0.1:${api_port}:${api_port}" \
    --env APP_ENV=staging \
    --env "DATABASE_URL=${container_database_url}" \
    --env "PORT=${api_port}" \
    "${image_tag}" api >/dev/null
containers+=("${api_container}")

docker run --detach --name "${worker_container}" --network "${network_name}" \
    --env APP_ENV=staging \
    --env "DATABASE_URL=${container_database_url}" \
    --env WORKER_HEARTBEAT_INTERVAL_SECONDS=1 \
    "${image_tag}" worker >/dev/null
containers+=("${worker_container}")

wait_for_http_200 /health
wait_for_http_200 /ready
wait_for_http_200 /version
wait_for_healthy_worker

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
