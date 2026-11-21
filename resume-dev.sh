#!/usr/bin/env bash

set -euo pipefail

INIT_CMD="pi"

BASE_SESSION="dev-$(printf '%s' "$PWD" | cksum | cut -d' ' -f1)"
SESSION="$BASE_SESSION"

if [[ "${1:-}" == "list" ]]; then
    exec rift list
fi

if [[ $# -gt 0 ]]; then
    SESSION="${BASE_SESSION}.${1}"
    shift
fi

session_alive() {
    rift list 2>/dev/null | grep -q "^name=${SESSION}	" && \
        ! rift list 2>/dev/null | grep -q "^name=${SESSION}	.*ended="
}

if rift list 2>/dev/null | grep -q "^name=${SESSION}	"; then
    if ! session_alive; then
        rift kill "$SESSION" 2>/dev/null || true
    fi
fi

if ! session_alive && [[ -n "$INIT_CMD" ]]; then
    rift attach -d "$SESSION" bash -lc "$INIT_CMD"
fi

if [[ $# -gt 0 ]]; then
    exec rift run "$SESSION" "$@"
else
    exec rift attach "$SESSION"
fi
