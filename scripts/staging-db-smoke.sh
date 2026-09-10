#!/usr/bin/env bash

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
migrations_dir="${repo_root}/services/backend/migrations"

fail() {
    echo "staging database smoke failed: $*" >&2
    exit 1
}

database_url="${DATABASE_URL:-}"
[[ -n "${database_url}" ]] || fail "DATABASE_URL is required"

command -v psql >/dev/null 2>&1 || fail "psql is required"

query() {
    local description="$1"
    local sql="$2"
    local result

    if ! result="$(
        PGCONNECT_TIMEOUT="${STAGING_DB_CONNECT_TIMEOUT_SECONDS:-10}" \
            psql "${database_url}" \
            --no-psqlrc \
            --tuples-only \
            --no-align \
            --set ON_ERROR_STOP=1 \
            --command "${sql}" \
            2>/dev/null
    )"; then
        fail "${description}"
    fi

    printf '%s' "${result}"
}

[[ "$(query "database connectivity check failed" "SELECT 1;")" == "1" ]] \
    || fail "database connectivity returned an unexpected result"

[[ "$(query "PostGIS extension check failed" \
    "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'postgis');")" == "t" ]] \
    || fail "PostGIS extension is not enabled"

[[ -n "$(query "PostGIS function check failed" "SELECT PostGIS_Version();")" ]] \
    || fail "PostGIS did not return a version"

mapfile -t migration_files < <(find "${migrations_dir}" -maxdepth 1 -type f -name '*.sql' -printf '%f\n' | sort)
(( ${#migration_files[@]} > 0 )) || fail "no repository migrations were found"

migration_versions=()
for migration_file in "${migration_files[@]}"; do
    migration_version="${migration_file%%_*}"
    [[ "${migration_version}" =~ ^[0-9]+$ ]] \
        || fail "migration filename does not begin with a numeric version"
    migration_versions+=("${migration_version}")
done
migration_version_list="$(IFS=,; printf '%s' "${migration_versions[*]}")"

[[ "$(query "SQLx migration history check failed" \
    "SELECT COUNT(*) FROM _sqlx_migrations WHERE success AND version IN (${migration_version_list});")" == "${#migration_versions[@]}" ]] \
    || fail "not every repository migration is applied successfully"

[[ "$(query "failed SQLx migration history check failed" \
    "SELECT COUNT(*) FROM _sqlx_migrations WHERE NOT success;")" == "0" ]] \
    || fail "SQLx migration history contains a failed migration"

query "service_heartbeats table is missing or not queryable" \
    "SELECT service_name, last_seen_at FROM service_heartbeats LIMIT 0;" >/dev/null

echo "staging database smoke passed"
