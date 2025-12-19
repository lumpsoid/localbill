#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# =============================================================================
# Script: insert_queued_links.sh
# Purpose: Fetch queued links from API and process each through parser/mapper
# Usage: ./insert_queued_links.sh
# 
# Environment Variables:
#   TRANSACTION_DIR      Output directory (default: ./data)
#   PARSER          Parser script path (default: ./scripts/parser/rs_parser.py)
#   MAPPER          Mapper script path (default: ./scripts/mapper/invoice_json_to_md.py)
#   QUEUED_LINKS    Queued links script (default: ./scripts/fetch/queued_links.sh)
#
# Requires:
#   - queued_links.sh script
#   - parser and mapper scripts
# =============================================================================
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"

# Configuration
CONFIG_LOADER="$PROJECT_ROOT/scripts/config/config_loader.sh"
QUEUED_LINKS="$PROJECT_ROOT/scripts/fetch/queued_links.sh"
QUEUE_CLEANER="$PROJECT_ROOT/scripts/fetch/queue_cleaner.sh"
PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

parser() {
    "$PYTHON" "$PROJECT_ROOT/scripts/parser/rs_parser.py" "$@"
}
mapper() {
    "$PYTHON" "$PROJECT_ROOT/scripts/mapper/invoice_json_to_md.py" "$@"
}

sync_data() {
    "$PROJECT_ROOT/bin/sync_data.sh" "$@"
}

# Exit codes
readonly EXIT_SUCCESS=0
readonly EXIT_MISSING_DEPENDENCY=1
readonly EXIT_FETCH_FAILED=2
readonly EXIT_PROCESSING_FAILED=3
readonly EXIT_NO_LINKS=4

# Check dependencies
check_dependencies() {
    local missing=0
    
    if [[ ! -x "$QUEUED_LINKS" && ! -f "$QUEUED_LINKS" ]]; then
        echo "Error: Queued links script not found: $QUEUED_LINKS" >&2
        missing=1
    fi

    if [[ ! -x "$QUEUE_CLEANER" && ! -f "$QUEUE_CLEANER" ]]; then
        echo "Error: Queue cleaner script not found: $QUEUE_CLEANER" >&2
        missing=1
    fi
    
    return $missing
}

# Fetch queued links
fetch_links() {
    "$QUEUED_LINKS" || return $EXIT_FETCH_FAILED
}

find_duplicate_links() {
    "$PROJECT_ROOT/scripts/search/find-duplicate-links.sh" "$@"
}

# Process a single link
process_link() {
    local link="$1"
    
    parser "$link" | mapper --stdin --output-dir "$TRANSACTION_DIR" 2>/dev/null
}

# Main execution
main() {
    source "$CONFIG_LOADER"
    TRANSACTION_DIR="${TRANSACTION_DIR:-$PROJECT_ROOT/data}"

    check_dependencies || exit $EXIT_MISSING_DEPENDENCY
    
    mkdir -p "$TRANSACTION_DIR"
    
    # Fetch links into array
    mapfile -t links < <(fetch_links)
    
    if [[ $? -ne 0 ]]; then
        echo "Error: Failed to fetch queued links" >&2
        exit $EXIT_FETCH_FAILED
    fi
    
    if [[ ${#links[@]} -eq 0 ]]; then
        echo "No queued links to process" >&2
        exit $EXIT_NO_LINKS
    fi
    
    local processed=0
    local failed=0
    local -a processed_links=()
    
    for link in "${links[@]}"; do
        # Skip empty lines
        [[ -z "$link" ]] && continue

        if find_duplicate_links "$link"; then
            echo "Duplicate found, skipping: $link" >&2
            processed_links+=("$link")
            continue
        fi
        
        if process_link "$link"; then
            ((++processed))
            processed_links+=("$link")
        else
            ((++failed))
            echo "Error: Failed to process: $link" >&2
        fi
    done
    
    # Summary to stderr
    echo "Processed: $processed, Failed: $failed" >&2

    if [[ ${#processed_links[@]} -gt 0 ]]; then
        printf '%s\n' "${processed_links[@]}" | "$QUEUE_CLEANER"
        if [[ $? -ne 0 ]]; then
            echo "Warning: Failed to clean processed links from queue" >&2
        fi
    fi

    sync_data
    
    # Exit with error if any failed
    [[ $failed -eq 0 ]] && exit $EXIT_SUCCESS || exit $EXIT_PROCESSING_FAILED
}

main "$@"
