#!/usr/bin/env bash

set -euo pipefail

# find-duplicate-links.sh
# Usage: TRANSACTION_DIR=/path/to/dir ./find-duplicate-links.sh "pattern"

readonly PATTERN="${1:?Error: Pattern argument required}"

if [[ -z "${TRANSACTION_DIR:-}" ]]; then
    echo "Error: TRANSACTION_DIR environment variable not set" >&2
    exit 1
fi

if [[ ! -d "$TRANSACTION_DIR" ]]; then
    echo "Error: Directory '$TRANSACTION_DIR' does not exist" >&2
    exit 1
fi

rg -F "$PATTERN" "$TRANSACTION_DIR" >/dev/null 
