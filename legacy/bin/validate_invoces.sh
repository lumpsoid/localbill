#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# Configuration
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"
CONFIG_LOADER="$PROJECT_ROOT/scripts/config/config_loader.sh"
PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

validater() {
    "$PYTHON" "$PROJECT_ROOT/scripts/validate/validate_invoices.py" "$@" 
}

main() {
    source "$CONFIG_LOADER"
    TRANSACTION_DIR="${TRANSACTION_DIR:-$PROJECT_ROOT/data}"

    # Create output directory if missing
    mkdir -p "$TRANSACTION_DIR"

    # Process the link
    validater --schema "$PROJECT_ROOT/schemas/schema.yaml" "$TRANSACTION_DIR" || {
        echo "Error: Validation failed" >&2
        exit 1
    }
}

main "$@"
