#!/usr/bin/env bash
set -euo pipefail

# Exit early if not Termux
if [[ -z "$TERMUX_VERSION" ]]; then
    exit 0
fi

echo "[Termux] Installing base dependencies..."

# Update packages
pkg update -y

# Core tools
pkg install -y \
    python \
    git \
    jq \
    curl

echo "✓ [Termux] Base dependencies installed"
