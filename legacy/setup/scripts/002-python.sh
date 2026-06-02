#!/usr/bin/env bash
set -euo pipefail

# Resolve project root
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/../.." && pwd )"

# Define venv path inside the project folder
VENV_DIR="$PROJECT_ROOT/.venv"

echo "Creating Python virtual environment in $VENV_DIR..."

# Check if Python is installed
if ! command -v python3 &> /dev/null; then
    echo "Python3 is not installed. Please install it first."
    exit 1
fi

# Create venv if it doesn't exist
if [[ ! -d "$VENV_DIR" ]]; then
    python3 -m venv "$VENV_DIR"
    echo "✓ Python virtual environment created."
else
    echo "✓ Python virtual environment already exists."
fi

echo "✓ Python venv is ready"

