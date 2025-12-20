#!/data/data/com.termux/files/usr/bin/bash

# ==============================================================================
# Termux Environment Wrapper
# This script ensures that standard Linux shebangs (#!/usr/bin/env) work 
# correctly within the Termux Android Intent environment.
# ==============================================================================

if [ -z "$1" ]; then
    echo "Usage: ./termux-env-launcher.sh <script_path> [args...]"
    exit 1
fi

# Inject the Termux-Exec library to handle shebang redirection
LD_PRELOAD="$PREFIX/lib/libtermux-exec.so" "$PREFIX/bin/bash" "$@"
