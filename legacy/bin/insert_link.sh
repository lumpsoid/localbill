#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# =============================================================================
# Script: insert_link.sh
# Purpose: Process a URL through parser and mapper into data directory
# Usage: ./insert_link.sh <link>
# Requires:
#   - ../scripts/parser/rs_parser.py
#   - ../scripts/mapper/invoice_json_to_md.py
# =============================================================================

# Configuration
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"
CONFIG_LOADER="$PROJECT_ROOT/scripts/config/config_loader.sh"
PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

parser() {
    "$PYTHON" "$PROJECT_ROOT/scripts/parser/rs_parser.py" "$@"
}
mapper() {
    "$PYTHON" "$PROJECT_ROOT/scripts/mapper/invoice_json_to_md.py" "$@"
}

find_duplicate_links() {
    "$PROJECT_ROOT/scripts/search/find-duplicate-links.sh" "$@"
}

sync_data() {
    "$PROJECT_ROOT/bin/sync_data.sh" "$@"
}

check_internet() {
  "$PROJECT_ROOT/scripts/fetch/check_internet.sh"
}

main() {
    # Argument check
    if [[ $# -lt 1 ]]; then
        echo "Error: Missing required URL argument." >&2
        echo "Usage: $0 <link>" >&2
        exit 1
    fi

    LINK="$1"

    # Basic validation
    if [[ ! "$LINK" =~ ^https?:// ]]; then
        echo "Error: Invalid URL: $LINK" >&2
        exit 1
    fi

    source "$CONFIG_LOADER"
    TRANSACTION_DIR="${TRANSACTION_DIR:-$PROJECT_ROOT/data}"

    # Create output directory if missing
    mkdir -p "$TRANSACTION_DIR"

    # Run processing pipeline
    echo "Processing link: $LINK" >&2

    if find_duplicate_links "$LINK"; then
        echo "Duplicate found, skipping" >&2
        exit 0
    fi
    
    if ! check_internet; then
        echo "No internet, queuing" >&2
        
        # Avoid duplicates
        if ! grep -Fxq "$LINK" "$QUEUE_FILE" 2>/dev/null; then
            echo "$LINK" >> "$QUEUE_FILE"

            sync_data
        fi
        
        exit 1 
    fi

    # Process the link
    parser "$LINK" | "$PROJECT_ROOT/scripts/sanitize/sanitize_rs.pl" | mapper --stdin --output-dir "$TRANSACTION_DIR" || {
      echo "Pipeline failed" >&2
      echo "$LINK" >> "$FAILED_LINKS"

      sync_data

      exit 1
    }

    sync_data

    echo "Success: Output written to $TRANSACTION_DIR"
}

main "$@"
