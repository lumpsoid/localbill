#!/usr/bin/env bash
set -eo pipefail

# Exit early if not Termux
if [[ ! -n "$TERMUX_VERSION" ]]; then
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
    curl \
    libxslt \
    ripgrep \
    perl

echo "✓ [Termux] Base dependencies installed"
