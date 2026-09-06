#!/usr/bin/env bash

set -Eeuo pipefail

binary="${1:?usage: dev-watch.sh <binary>}"
watch_pid=""
shutdown_requested=false

forward_shutdown() {
    shutdown_requested=true

    if [[ -n "${watch_pid}" ]] && kill -0 "${watch_pid}" 2>/dev/null; then
        kill -TERM "${watch_pid}"
    fi
}

trap forward_shutdown INT TERM

cargo watch -x "run --bin ${binary}" &
watch_pid="$!"

set +e
wait "${watch_pid}"
watch_status="$?"
set -e

if [[ "${shutdown_requested}" == "true" ]]; then
    wait "${watch_pid}" 2>/dev/null || true
    exit 0
fi

exit "${watch_status}"
