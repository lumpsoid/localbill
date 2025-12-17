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
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"

# Configuration
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/data}"

PYTHON="$PROJECT_ROOT/scripts/wrapper/python-run.sh"

parser() {
    "$PYTHON" "$PROJECT_ROOT/scripts/parser/rs_parser.py" "$@"
}
mapper() {
    "$PYTHON" "$PROJECT_ROOT/scripts/mapper/invoice_json_to_md.py" "$@"
}




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

# Create output directory if missing
mkdir -p "$OUTPUT_DIR"

# Run processing pipeline
{
    echo "Processing link: $LINK"
    parser "$LINK" | mapper --stdin --output-dir "$OUTPUT_DIR"
} || {
    echo "Error: Processing failed for $LINK" >&2
    exit 1
}

echo "Success: Output written to $OUTPUT_DIR"

