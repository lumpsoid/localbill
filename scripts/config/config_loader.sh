#!/bin/bash

XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
CONFIG_FILE="$XDG_CONFIG_HOME/localbills/config"

if [ -f "$CONFIG_FILE" ]; then
    # Export variables while ignoring comments and empty lines
    set -a
    source <(grep -v '^#' "$CONFIG_FILE" | sed 's/^/export /')
    set +a
else
    echo "Error: Config file not found at $CONFIG_FILE" >&2
    exit 1
fi
