#!/usr/bin/env bash

set -Eeuo pipefail

smoke_base_url=""
smoke_temp_dir=""
smoke_access_token=""
last_http_status=""
last_http_version=""

is_https_base_url() {
    [[ "$1" =~ ^https://[^/?#@[:space:]]+/?$ ]]
}

is_uuid() {
    [[ "$1" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$ ]]
}

is_http2_or_higher() {
    [[ "$1" == "2" || "$1" == "3" ]]
}

header_value() {
    local header_name="${1,,}"

    awk -v header_name="${header_name}:" '
        tolower($1) == header_name {
            value = $2
            sub(/\r$/, "", value)
            print value
            exit
        }
    '
}

backend_version() {
    local manifest_path="${1:-services/backend/Cargo.toml}"

    awk -F '"' '
        /^\[package\]$/ { in_package = 1; next }
        in_package && /^\[/ { exit }
        in_package && /^version = "/ { print $2; exit }
    ' "${manifest_path}"
}

access_token_from_response() {
    jq --exit-status --raw-output '
        if (.access_token | type) == "string" and (.access_token | length) > 0
        then .access_token
        else error("missing usable access token")
        end
    ' "$1"
}

fail() {
    echo "staging runtime smoke failed: $*" >&2
    exit 1
}

cleanup() {
    if [[ -n "${smoke_temp_dir}" && -d "${smoke_temp_dir}" ]]; then
        rm -rf "${smoke_temp_dir}"
    fi
}

perform_request() {
    local path="$1"
    local correlation_id="${2:-}"
    local access_token="${3:-}"
    local curl_arguments=(
        --silent
        --show-error
        --http2
        --connect-timeout 5
        --max-time 10
        --output "${smoke_temp_dir}/response-body.json"
        --dump-header "${smoke_temp_dir}/response-headers.txt"
        --write-out "%{http_code} %{http_version}\n"
    )

    if [[ -n "${correlation_id}" ]]; then
        curl_arguments+=(--header "x-correlation-id: ${correlation_id}")
    fi

    if [[ -n "${access_token}" ]]; then
        printf 'Authorization: Bearer %s\n' "${access_token}" \
            >"${smoke_temp_dir}/request-authorization-header.txt"
        curl_arguments+=(--header "@${smoke_temp_dir}/request-authorization-header.txt")
    fi

    if ! curl "${curl_arguments[@]}" "${smoke_base_url}${path}" \
        >"${smoke_temp_dir}/response-metrics.txt" \
        2>"${smoke_temp_dir}/curl-error.txt"; then
        return 1
    fi

    read -r last_http_status last_http_version <"${smoke_temp_dir}/response-metrics.txt"
}

wait_for_response() {
    local path="$1"
    local expected_body="$2"
    local description="$3"
    local attempts="$4"
    local access_token="${5:-}"
    local attempt

    for ((attempt = 1; attempt <= attempts; attempt++)); do
        if perform_request "${path}" "" "${access_token}" \
            && [[ "${last_http_status}" == "200" ]] \
            && grep --fixed-strings --quiet "${expected_body}" \
                "${smoke_temp_dir}/response-body.json"; then
            return
        fi

        if ((attempt < attempts)); then
            sleep 2
        fi
    done

    fail "${description} did not become healthy within the bounded retry period"
}

authenticate_smoke_user() {
    local auth_url auth_status

    auth_url="${STAGING_SUPABASE_URL%/}/auth/v1/token?grant_type=password"

    jq --null-input \
        '{email: env.STAGING_SMOKE_EMAIL, password: env.STAGING_SMOKE_PASSWORD}' \
        >"${smoke_temp_dir}/supabase-auth-request.json"

    if ! auth_status="$(
        curl \
            --silent \
            --show-error \
            --connect-timeout 5 \
            --max-time 10 \
            --output "${smoke_temp_dir}/supabase-auth-response.json" \
            --write-out '%{http_code}' \
            --header "apikey: ${STAGING_SUPABASE_PUBLISHABLE_KEY}" \
            --header 'Content-Type: application/json' \
            --data-binary "@${smoke_temp_dir}/supabase-auth-request.json" \
            "${auth_url}"
    )"; then
        fail "Supabase smoke-user authentication request failed"
    fi

    [[ "${auth_status}" == "200" ]] \
        || fail "Supabase smoke-user authentication was rejected"

    if ! smoke_access_token="$(
        access_token_from_response "${smoke_temp_dir}/supabase-auth-response.json"
    )"; then
        fail "Supabase smoke-user authentication response did not contain a usable access token"
    fi
}

main() {
    local request_id correlation_id expected_version

    command -v curl >/dev/null 2>&1 || fail "curl is required"
    command -v jq >/dev/null 2>&1 || fail "jq is required"

    smoke_base_url="${STAGING_API_BASE_URL:-}"
    [[ -n "${smoke_base_url}" ]] || fail "STAGING_API_BASE_URL is required"
    is_https_base_url "${smoke_base_url}" \
        || fail "STAGING_API_BASE_URL must be an HTTPS origin without credentials or a path"
    smoke_base_url="${smoke_base_url%/}"

    [[ -n "${STAGING_SUPABASE_URL:-}" ]] \
        || fail "STAGING_SUPABASE_URL is required"
    is_https_base_url "${STAGING_SUPABASE_URL}" \
        || fail "STAGING_SUPABASE_URL must be an HTTPS origin without credentials or a path"
    [[ -n "${STAGING_SUPABASE_PUBLISHABLE_KEY:-}" ]] \
        || fail "STAGING_SUPABASE_PUBLISHABLE_KEY is required"
    [[ -n "${STAGING_SMOKE_EMAIL:-}" ]] \
        || fail "STAGING_SMOKE_EMAIL is required"
    [[ -n "${STAGING_SMOKE_PASSWORD:-}" ]] \
        || fail "STAGING_SMOKE_PASSWORD is required"

    expected_version="$(backend_version)"
    [[ -n "${expected_version}" ]] || fail "backend package version could not be determined"

    smoke_temp_dir="$(mktemp -d)"
    trap cleanup EXIT

    wait_for_response /health '"status":"ok"' "API health" 20
    is_http2_or_higher "${last_http_version}" \
        || fail "public endpoint negotiated HTTP/${last_http_version}, expected HTTP/2 or HTTP/3"

    request_id="$(header_value x-request-id <"${smoke_temp_dir}/response-headers.txt")"
    is_uuid "${request_id}" || fail "x-request-id is missing or is not a valid UUID"

    correlation_id="agro-ops-staging-smoke"
    perform_request /health "${correlation_id}" \
        || fail "correlation ID propagation request failed"
    [[ "${last_http_status}" == "200" ]] \
        || fail "correlation ID propagation request did not return HTTP 200"
    [[ "$(header_value x-correlation-id <"${smoke_temp_dir}/response-headers.txt")" == "${correlation_id}" ]] \
        || fail "x-correlation-id was not propagated unchanged"

    wait_for_response /ready '"status":"ready"' "API readiness" 20
    wait_for_response /version '"service":"agro-ops-backend"' "API version endpoint" 5
    grep --fixed-strings --quiet "\"version\":\"${expected_version}\"" \
        "${smoke_temp_dir}/response-body.json" \
        || fail "deployed backend version does not match ${expected_version}"

    authenticate_smoke_user

    perform_request /internal/worker/status \
        || fail "anonymous worker status request failed"
    [[ "${last_http_status}" == "401" ]] \
        || fail "anonymous worker status request did not return HTTP 401"

    wait_for_response \
        /internal/worker/status \
        '"status":"healthy"' \
        "worker heartbeat" \
        30 \
        "${smoke_access_token}"

    echo "staging runtime smoke passed"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
