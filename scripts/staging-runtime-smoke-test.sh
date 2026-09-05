#!/usr/bin/env bash

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

source scripts/staging-runtime-smoke.sh

fail_test() {
    echo "staging runtime smoke unit test failed: $*" >&2
    exit 1
}

is_https_base_url "https://api.example.test" || fail_test "valid HTTPS origin was rejected"
is_https_base_url "https://api.example.test/" || fail_test "valid trailing slash was rejected"
if is_https_base_url "http://api.example.test"; then
    fail_test "plain HTTP origin was accepted"
fi
if is_https_base_url "https://user:password@api.example.test"; then
    fail_test "origin containing credentials was accepted"
fi
if is_https_base_url "https://api.example.test/base-path"; then
    fail_test "origin containing a path was accepted"
fi

valid_uuid="123e4567-e89b-42d3-a456-426614174000"
is_uuid "${valid_uuid}" || fail_test "valid UUID was rejected"
if is_uuid "123e4567-e89b-02d3-a456-426614174000"; then
    fail_test "UUID with an invalid version was accepted"
fi

is_http2_or_higher "2" || fail_test "HTTP/2 was rejected"
is_http2_or_higher "3" || fail_test "HTTP/3 was rejected"
if is_http2_or_higher "1.1"; then
    fail_test "HTTP/1.1 was accepted"
fi

parsed_header="$(
    printf 'HTTP/2 200\r\nX-Request-ID: %s\r\n\r\n' "${valid_uuid}" \
        | header_value x-request-id
)"
[[ "${parsed_header}" == "${valid_uuid}" ]] \
    || fail_test "case-insensitive response header parsing failed"

parsed_version="$(
    backend_version <(
        printf '[package]\nname = "fixture"\nversion = "9.8.7"\n\n[dependencies]\n'
    )
)"
[[ "${parsed_version}" == "9.8.7" ]] \
    || fail_test "backend package version parsing failed"

missing_version="$(backend_version <(printf '[dependencies]\n'))"
[[ -z "${missing_version}" ]] \
    || fail_test "manifest without a package version returned a value"

smoke_test_temp_dir="$(mktemp -d)"
trap 'rm -rf "${smoke_test_temp_dir}"' EXIT
printf '%s\n' '{"access_token":"test-access-token"}' >"${smoke_test_temp_dir}/valid-auth.json"
[[ "$(access_token_from_response "${smoke_test_temp_dir}/valid-auth.json")" == "test-access-token" ]] \
    || fail_test "access token was not parsed from a valid Supabase response"

printf '%s\n' '{"access_token":""}' >"${smoke_test_temp_dir}/invalid-auth.json"
if access_token_from_response "${smoke_test_temp_dir}/invalid-auth.json" >/dev/null 2>&1; then
    fail_test "empty access token was accepted"
fi

smoke_temp_dir="${smoke_test_temp_dir}"
export STAGING_SUPABASE_URL="https://project.supabase.co"
export STAGING_SUPABASE_PUBLISHABLE_KEY="sb_publishable_test"
export STAGING_SMOKE_EMAIL="smoke@example.test"
export STAGING_SMOKE_PASSWORD="fixture-password"
curl() {
    local argument output_path=""
    local next_is_output=false

    for argument in "$@"; do
        if [[ "${next_is_output}" == true ]]; then
            output_path="${argument}"
            next_is_output=false
        elif [[ "${argument}" == "--output" ]]; then
            next_is_output=true
        fi
    done

    printf '%s\n' '{"access_token":"fresh-test-access-token"}' >"${output_path}"
    printf '200'
}

authenticate_smoke_user
[[ "${smoke_access_token}" == "fresh-test-access-token" ]] \
    || fail_test "fresh Supabase access token was not retained for private requests"
unset -f curl

echo "staging runtime smoke unit tests passed"
