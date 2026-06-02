#!/usr/bin/env bash

CACHE_FILE="${TMPDIR:-/tmp}/internet_status"

NOW=$(date +%s)

# Get modification time. If file doesn't exist, stat fails and we set it to 0.
MOD_TIME=$(stat -c %Y "$CACHE_FILE" 2>/dev/null || echo 0)

# 1. If MOD_TIME is 0, the file doesn't exist.
# 2. If NOW - MOD_TIME >= 10, the cache is expired.
# Only enter this block if the file exists AND is fresh.
if (( MOD_TIME != 0 )) && (( NOW - MOD_TIME < 10 )); then
    exit $(<"$CACHE_FILE")
fi

# Perform the fast network probe (TCP touch on Cloudflare DNS)
if timeout 0.2 bash -c "</dev/tcp/1.1.1.1/53" &>/dev/null; then
    RESULT=0
else
    RESULT=1
fi

# Update the cache and exit
echo "$RESULT" > "$CACHE_FILE"
exit "$RESULT"
