#!/usr/bin/env bash
set -euo pipefail

# Detect platform
PLATFORM=""
if [[ -n "${TERMUX_VERSION:-}" ]]; then
    PLATFORM="termux"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    PLATFORM="macos"
elif [[ -f /etc/arch-release ]]; then
    PLATFORM="arch"
else
    PLATFORM="linux"
fi

# Exit early if not Termux
if [[ "$PLATFORM" != "termux" ]]; then
    exit 0
fi

echo "[Termux] Installing base dependencies..."

# Update packages
pkg update -y

# Core tools
pkg install -y \
    python \
    git \
    tree \
    jq

echo "✓ [Termux] Base dependencies installed"
