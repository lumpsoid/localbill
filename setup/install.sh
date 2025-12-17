#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "========================================"
echo "Invoice Project Setup"
echo "========================================"
echo

for script in "$SCRIPT_DIR/scripts"/*.sh; do
    bash "$script"
done

echo
echo "========================================"
echo "✓ Installation complete!"
echo "========================================"
