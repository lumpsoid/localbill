#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# =============================================================================
# Script: insert_queued_links.sh
# Purpose: Fetch queued links from API and process each through parser/mapper
# Usage: ./insert_queued_links.sh
# 
# Environment Variables:
#   OUTPUT_DIR      Output directory (default: ./data)
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
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/data}"

QUEUED_LINKS="$PROJECT_ROOT/scripts/fetch/queued_links.sh"
PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

parser() {
    "$PYTHON" "$PROJECT_ROOT/scripts/parser/rs_parser.py" "$@"
}
mapper() {
    "$PYTHON" "$PROJECT_ROOT/scripts/mapper/invoice_json_to_md.py" "$@"
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
    
    return $missing
}

# Fetch queued links
fetch_links() {
    "$QUEUED_LINKS" || return $EXIT_FETCH_FAILED
}

# Process a single link
process_link() {
    local link="$1"
    
    parser "$link" | mapper --stdin --output-dir "$OUTPUT_DIR" 2>/dev/null
}

# Main execution
main() {
    check_dependencies || exit $EXIT_MISSING_DEPENDENCY
    
    mkdir -p "$OUTPUT_DIR"
    
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
    
    for link in "${links[@]}"; do
        # Skip empty lines
        [[ -z "$link" ]] && continue
        
        if process_link "$link"; then
            ((processed++))
            echo "$link"
        else
            ((failed++))
            echo "Error: Failed to process: $link" >&2
        fi
    done
    
    # Summary to stderr
    echo "Processed: $processed, Failed: $failed" >&2
    
    # Exit with error if any failed
    [[ $failed -eq 0 ]] && exit $EXIT_SUCCESS || exit $EXIT_PROCESSING_FAILED
}

main "$@"
