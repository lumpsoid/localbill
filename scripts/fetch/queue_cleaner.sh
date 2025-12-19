#!/usr/bin/env bash
set -euo pipefail
# =============================================================================
# Purpose: Send DELETE request to API with items to remove
# Usage: ./script.sh <item1> [item2] [item3] ...
#        echo -e "item1\nitem2" | ./delete_items.sh
#
# Environment Variables:
#   API_HOST        API host (default: 192.168.1.2)
#   API_PORT        API port (default: 8087)
#   API_ENDPOINT    API endpoint (default: /queue)
# =============================================================================

# Configuration
API_HOST="${API_HOST:-192.168.1.2}"
API_PORT="${API_PORT:-8087}"
API_ENDPOINT="${API_ENDPOINT:-/queue}"

# Backward compatibility with old env vars
BASE_URL="${BASE_URL:-http://${API_HOST}:${API_PORT}}"

# Exit codes
readonly EXIT_SUCCESS=0
readonly EXIT_NO_ITEMS=1
readonly EXIT_DELETE_FAILED=2

# Build URL
build_url() {
    local endpoint="$API_ENDPOINT"
    [[ "$endpoint" != /* ]] && endpoint="/$endpoint"
    echo "${BASE_URL}${endpoint}"
}

# Build JSON payload
build_payload() {
    local items=("$@")
    local json_array
    
    # Convert bash array to JSON array
    json_array=$(printf '%s\n' "${items[@]}" | jq -R . | jq -s .)
    
    # Build DeleteRequest JSON structure
    jq -n --argjson items "$json_array" '{items: $items}'
}

# Read items from stdin or args
read_items() {
    local items=()
    
    # Check if stdin has data
    if [[ ! -t 0 ]]; then
        # Read from stdin
        while IFS= read -r line; do
            [[ -n "$line" ]] && items+=("$line")
        done
    fi
    
    # Add command line arguments
    items+=("$@")
    
    # Remove duplicates while preserving order
    printf '%s\n' "${items[@]}" | awk '!seen[$0]++'
}

# Main execution
main() {
    local url
    local -a items
    
    # Read items from stdin and/or arguments
    mapfile -t items < <(read_items "$@")
    
    # Check if we have items to delete
    if [[ ${#items[@]} -eq 0 ]]; then
        echo "Error: No items provided" >&2
        echo "Usage: $0 <item1> [item2] [item3] ..." >&2
        echo "   or: echo -e \"item1\\nitem2\" | $0" >&2
        exit $EXIT_NO_ITEMS
    fi
    
    url=$(build_url)
    
    # Build JSON payload and send DELETE request
    local payload
    payload=$(build_payload "${items[@]}")
    
    curl -sf -X DELETE \
        -H "Content-Type: application/json" \
        -d "$payload" \
        "$url" || {
        echo "Error: DELETE request failed" >&2
        exit $EXIT_DELETE_FAILED
    }
}

main "$@"
