#!/usr/bin/env bash

CACHE_FILE="${TMPDIR:-/tmp}/internet_status"

# If cache is less than 60 seconds old, use it
if [[ -f "$CACHE_FILE" ]] && [[ $(find "$CACHE_FILE" -mmin -1) ]]; then
    exit $(cat "$CACHE_FILE")
fi

# Otherwise, check and update cache
if timeout 0.2 bash -c "</dev/tcp/1.1.1.1/53" &>/dev/null; then
    echo 0 > "$CACHE_FILE"
    exit 0
else
    echo 1 > "$CACHE_FILE"
    exit 1
fi
