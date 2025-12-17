#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/../.." &> /dev/null && pwd )"
REQUIREMENT_FILE="$PROJECT_ROOT/setup/requirements/base.txt"
PYTHON_PIP="$PROJECT_ROOT/.venv/bin/pip3"

[[ -f "$REQUIREMENT_FILE" ]] || { echo "Requirements file not found: $REQUIREMENT_FILE"; exit 1; }


echo "Installing python dependencies..."

"$PYTHON_PIP" install -r "$REQUIREMENT_FILE"

echo "✓ Python dependencies installed"
