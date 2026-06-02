#!/usr/bin/env bash
set -euo pipefail

# Exit early if not Arch Linux
if [[ ! -e /etc/arch-release ]]; then
    exit 0
fi

echo "[Arch] Installing base dependencies..."

# Core system packages
sudo pacman -Sy --needed --noconfirm \
    python \
    git \
    jq

echo "✓ [Arch] Base dependencies installed"
