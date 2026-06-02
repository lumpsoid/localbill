#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/../.." &> /dev/null && pwd )"
VENV_DIR="$PROJECT_ROOT/.venv"
PYTHON_BIN="$VENV_DIR/bin/python"

# Ensure venv exists
if [[ ! -x "$PYTHON_BIN" ]]; then
    echo "Virtual environment not found."
    exit 1
fi

# Execute Python script with all arguments
exec "$PYTHON_BIN" "$@"

