#!/usr/bin/env bash

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

source scripts/validate-railway-api-config.sh

fail_test() {
    echo "Railway API configuration preflight test failed: $*" >&2
    exit 1
}

assert_no_secret_values() {
    local output_path="$1"

    for secret in \
        "DATABASE_URL_DO_NOT_LEAK_7f4f" \
        "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f" \
        "SECRET_KEY_DO_NOT_LEAK_7f4f" \
        "INVITE_REDIRECT_DO_NOT_LEAK_7f4f"
    do
        if grep --fixed-strings --quiet "${secret}" "${output_path}"; then
            fail_test "validator output exposed a synthetic secret"
        fi
    done
}

assert_failure_with_missing_names() {
    local input_path="$1"
    local output_path="$2"
    shift 2
    local variable_name

    if validate_railway_api_config "${input_path}" >"${output_path}" 2>&1; then
        fail_test "incomplete configuration was accepted"
    fi
    for variable_name in "$@"; do
        grep --fixed-strings --quiet -- "- ${variable_name}" "${output_path}" \
            || fail_test "missing variable name ${variable_name} was not reported"
    done
    assert_no_secret_values "${output_path}"
}

preflight_test_temp_dir="$(mktemp -d)"
trap 'rm -rf "${preflight_test_temp_dir}"' EXIT

valid_input="${preflight_test_temp_dir}/valid.json"
printf '%s\n' '{
  "DATABASE_URL": "postgres://DATABASE_URL_DO_NOT_LEAK_7f4f@db.example.test/agro_ops",
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_SECRET_KEY": "SECRET_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_INVITE_REDIRECT_URL": "https://INVITE_REDIRECT_DO_NOT_LEAK_7f4f.example.test/aceptar-invitacion",
  "SUPABASE_STORAGE_BUCKET": "documentos_privados"
}' >"${valid_input}"

valid_output="${preflight_test_temp_dir}/valid-output.txt"
validate_railway_api_config "${valid_input}" >"${valid_output}" 2>&1 \
    || fail_test "complete API configuration was rejected"
[[ ! -s "${valid_output}" ]] || fail_test "complete configuration produced unexpected output"

missing_secret_input="${preflight_test_temp_dir}/missing-secret.json"
printf '%s\n' '{
  "DATABASE_URL": "postgres://DATABASE_URL_DO_NOT_LEAK_7f4f@db.example.test/agro_ops",
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_INVITE_REDIRECT_URL": "https://INVITE_REDIRECT_DO_NOT_LEAK_7f4f.example.test/aceptar-invitacion",
  "SUPABASE_STORAGE_BUCKET": "documentos_privados"
}' >"${missing_secret_input}"
assert_failure_with_missing_names \
    "${missing_secret_input}" \
    "${preflight_test_temp_dir}/missing-secret-output.txt" \
    SUPABASE_SECRET_KEY

missing_redirect_input="${preflight_test_temp_dir}/missing-redirect.json"
printf '%s\n' '{
  "DATABASE_URL": "postgres://DATABASE_URL_DO_NOT_LEAK_7f4f@db.example.test/agro_ops",
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_SECRET_KEY": "SECRET_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_STORAGE_BUCKET": "documentos_privados"
}' >"${missing_redirect_input}"
assert_failure_with_missing_names \
    "${missing_redirect_input}" \
    "${preflight_test_temp_dir}/missing-redirect-output.txt" \
    SUPABASE_INVITE_REDIRECT_URL

missing_multiple_input="${preflight_test_temp_dir}/missing-multiple.json"
printf '%s\n' '{
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f"
}' >"${missing_multiple_input}"
assert_failure_with_missing_names \
    "${missing_multiple_input}" \
    "${preflight_test_temp_dir}/missing-multiple-output.txt" \
    DATABASE_URL \
    SUPABASE_SECRET_KEY \
    SUPABASE_INVITE_REDIRECT_URL \
    SUPABASE_STORAGE_BUCKET

empty_value_input="${preflight_test_temp_dir}/empty-value.json"
printf '%s\n' '{
  "DATABASE_URL": "postgres://DATABASE_URL_DO_NOT_LEAK_7f4f@db.example.test/agro_ops",
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_SECRET_KEY": "   ",
  "SUPABASE_INVITE_REDIRECT_URL": "https://INVITE_REDIRECT_DO_NOT_LEAK_7f4f.example.test/aceptar-invitacion",
  "SUPABASE_STORAGE_BUCKET": "documentos_privados"
}' >"${empty_value_input}"
assert_failure_with_missing_names \
    "${empty_value_input}" \
    "${preflight_test_temp_dir}/empty-value-output.txt" \
    SUPABASE_SECRET_KEY

missing_storage_input="${preflight_test_temp_dir}/missing-storage.json"
printf '%s\n' '{
  "DATABASE_URL": "postgres://DATABASE_URL_DO_NOT_LEAK_7f4f@db.example.test/agro_ops",
  "APP_ENV": "staging",
  "SUPABASE_URL": "https://project.supabase.co",
  "SUPABASE_PUBLISHABLE_KEY": "PUBLISHABLE_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_SECRET_KEY": "SECRET_KEY_DO_NOT_LEAK_7f4f",
  "SUPABASE_INVITE_REDIRECT_URL": "https://INVITE_REDIRECT_DO_NOT_LEAK_7f4f.example.test/aceptar-invitacion"
}' >"${missing_storage_input}"
assert_failure_with_missing_names \
    "${missing_storage_input}" \
    "${preflight_test_temp_dir}/missing-storage-output.txt" \
    SUPABASE_STORAGE_BUCKET

echo "Railway API configuration preflight tests passed"
