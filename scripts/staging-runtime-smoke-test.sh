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

echo "staging runtime smoke unit tests passed"
