#!/usr/bin/env bash

set -Eeuo pipefail

readonly RAILWAY_API_REQUIRED_VARIABLES=(
    DATABASE_URL
    APP_ENV
    SUPABASE_URL
    SUPABASE_PUBLISHABLE_KEY
    SUPABASE_SECRET_KEY
    SUPABASE_INVITE_REDIRECT_URL
    SUPABASE_STORAGE_BUCKET
)

validate_railway_api_config() {
    local variables_json_path="$1"
    local required_variables_json missing_variables

    command -v jq >/dev/null 2>&1 || {
        echo "Railway API configuration validation requires jq." >&2
        return 2
    }
    [[ -f "${variables_json_path}" ]] || {
        echo "Railway API configuration could not be validated." >&2
        return 2
    }

    required_variables_json="$(printf '%s\n' "${RAILWAY_API_REQUIRED_VARIABLES[@]}" | jq -R . | jq -s .)"
    if ! missing_variables="$(
        jq --raw-output --argjson required "${required_variables_json}" '
            def has_nonempty_value($name):
                .[$name]? as $value
                | if ($value | type) == "string" then
                    ($value | gsub("^[[:space:]]+|[[:space:]]+$"; "") | length) > 0
                  elif ($value | type) == "object" and ($value.value? | type) == "string" then
                    ($value.value | gsub("^[[:space:]]+|[[:space:]]+$"; "") | length) > 0
                  else
                    false
                  end;
            if type != "object" then error("expected variables object") else . end
            | . as $variables
            | $required[] as $name
            | select($variables | has_nonempty_value($name) | not)
            | $name
        ' "${variables_json_path}" 2>/dev/null
    )"; then
        echo "Railway API configuration could not be validated." >&2
        return 2
    fi

    if [[ -n "${missing_variables}" ]]; then
        echo "Staging API configuration incomplete:" >&2
        while IFS= read -r variable_name; do
            printf '%s\n' "- ${variable_name}" >&2
        done <<<"${missing_variables}"
        echo "Deployment aborted before Railway deployment." >&2
        return 1
    fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    [[ "$#" -eq 1 ]] || {
        echo "Usage: $0 <railway-variables.json>" >&2
        exit 2
    }

    validate_railway_api_config "$1"
fi
