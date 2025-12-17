#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Script: queued_links.sh
# Purpose: Fetch queued links from API
# Usage: ./queued_links.sh
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
readonly EXIT_FETCH_FAILED=1

# Build URL
build_url() {
    local endpoint="$API_ENDPOINT"
    [[ "$endpoint" != /* ]] && endpoint="/$endpoint"
    echo "${BASE_URL}${endpoint}"
}

# Main execution
main() {
    local url
    url=$(build_url)
    
    curl -sf "$url" | jq -r '.items[].item' | sort -u || exit $EXIT_FETCH_FAILED
}

main "$@"
