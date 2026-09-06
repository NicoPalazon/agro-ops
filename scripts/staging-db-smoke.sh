#!/usr/bin/env bash

set -Eeuo pipefail

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

[[ "$(query "SQLx migration history check failed" \
    "SELECT COUNT(*) FROM _sqlx_migrations WHERE success AND version IN (20260903000000, 20260903010000);")" == "2" ]] \
    || fail "required Walking Skeleton migrations are not applied"

query "service_heartbeats table is missing or not queryable" \
    "SELECT service_name, last_seen_at FROM service_heartbeats LIMIT 0;" >/dev/null

echo "staging database smoke passed"
