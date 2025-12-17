#!/usr/bin/env bash
set -euo pipefail

# Exit early if not macOS
if [[ "$OSTYPE" != "darwin"* ]]; then
    exit 0
fi

echo "[macOS] Installing base dependencies..."

# Install Homebrew if needed
if ! command -v brew >/dev/null 2>&1; then
    echo "No homebrew installed..."
    exit 1
fi

# Core tools
brew install \
    python@3.11 \
    git \
    jq

echo "✓ [macOS] Base dependencies installed"
