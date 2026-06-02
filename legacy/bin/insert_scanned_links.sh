#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
# =============================================================================
# Script: insert_scanned_links.sh
# Purpose: Process links from scans file through parser/mapper
# Usage: ./insert_scanned_links.sh
# 
# Environment Variables:
#   TRANSACTION_DIR      Output directory (default: ./data)
#   PARSER          Parser script path (default: ./scripts/parser/rs_parser.py)
#   MAPPER          Mapper script path (default: ./scripts/mapper/invoice_json_to_md.py)
#
# Requires:
#   - parser and mapper scripts
#   - scans.txt file in ~/downloads/
# =============================================================================

PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"

# Configuration
TRANSACTION_DIR="${TRANSACTION_DIR:-$PROJECT_ROOT/data/transactions}"
DATA_DIR="${DATA_DIR:-$PROJECT_ROOT/data}"

SCANS_FILE="${1:-$HOME/downloads/scans.txt}"
FAILED_LINKS="$DATA_DIR/failed_links.txt"

CONFIG_LOADER="$PROJECT_ROOT/scripts/config/config_loader.sh"
PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

parser() {
    "$PYTHON" "$PROJECT_ROOT/scripts/parser/rs_parser.py" "$@" | "$PROJECT_ROOT/scripts/sanitize/sanitize_rs.pl"
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
readonly EXIT_NO_SCANS_FILE=2
readonly EXIT_PROCESSING_FAILED=3

# Check dependencies
check_dependencies() {
    local missing=0
    
    if [[ ! -f "$SCANS_FILE" ]]; then
        echo "Error: Scans file not found: $SCANS_FILE" >&2
        return 1
    fi
    
    return 0
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
    
    check_dependencies || exit $EXIT_NO_SCANS_FILE
    
    mkdir -p "$DATA_DIR"
    mkdir -p "$TRANSACTION_DIR"
    mkdir -p "$(dirname "$FAILED_LINKS")"
    
    # Read links from file into array
    mapfile -t links < "$SCANS_FILE"
    
    if [[ ${#links[@]} -eq 0 ]]; then
        echo "No links to process in $SCANS_FILE" >&2
        exit $EXIT_SUCCESS
    fi
    
    local processed=0
    local failed=0
    local -a failed_links=()
    
    for link in "${links[@]}"; do
        # Skip empty lines
        [[ -z "$link" ]] && continue
        
        if find_duplicate_links "$link"; then
            echo "Duplicate found, skipping: $link" >&2
            ((++processed))
            continue
        fi
        
        if process_link "$link"; then
            ((++processed))
            echo "Successfully processed: $link" >&2
        else
            ((++failed))
            echo "Error: Failed to process: $link" >&2
            failed_links+=("$link")
        fi
    done
    
    # Summary to stderr
    echo "Processed: $processed, Failed: $failed" >&2
    
    # Handle results
    if [[ $failed -eq 0 ]]; then
        # All links processed successfully, delete scans file
        rm -f "$SCANS_FILE"
        echo "All links processed successfully. Deleted $SCANS_FILE" >&2
    else
        # Append failed links to failed_links.txt and delete scans file
        printf '%s\n' "${failed_links[@]}" >> "$FAILED_LINKS"
        rm -f "$SCANS_FILE"
        echo "Failed links appended to $FAILED_LINKS. Deleted $SCANS_FILE" >&2
    fi
    
    sync_data
    
    # Exit with error if any failed
    [[ $failed -eq 0 ]] && exit $EXIT_SUCCESS || exit $EXIT_PROCESSING_FAILED
}

main "$@"
