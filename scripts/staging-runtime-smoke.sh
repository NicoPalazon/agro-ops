#!/usr/bin/env bash

set -Eeuo pipefail

smoke_base_url=""
smoke_temp_dir=""
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
    local attempt

    for ((attempt = 1; attempt <= attempts; attempt++)); do
        if perform_request "${path}" \
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

main() {
    local request_id correlation_id expected_version

    command -v curl >/dev/null 2>&1 || fail "curl is required"

    smoke_base_url="${STAGING_API_BASE_URL:-}"
    [[ -n "${smoke_base_url}" ]] || fail "STAGING_API_BASE_URL is required"
    is_https_base_url "${smoke_base_url}" \
        || fail "STAGING_API_BASE_URL must be an HTTPS origin without credentials or a path"
    smoke_base_url="${smoke_base_url%/}"

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

    wait_for_response /internal/worker/status '"status":"healthy"' "worker heartbeat" 30

    echo "staging runtime smoke passed"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
