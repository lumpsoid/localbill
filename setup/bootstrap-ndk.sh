#!/usr/bin/env bash
#
# bootstrap-ndk.sh — download & unpack the Android NDK for cross-compiling
# localbill to aarch64-linux-android (see README "Cross-compiling for Android").
#
# It reads Google's machine-readable package manifest
#   https://dl.google.com/android/repository/repository2-3.xml
# (the same file `sdkmanager` consumes) to discover the latest *stable* NDK,
# downloads the Linux archive, verifies its SHA-1, and unzips it.
#
# Usage:
#   build/bootstrap-ndk.sh                 # auto-detect latest stable NDK
#   NDK_VERSION=r28c build/bootstrap-ndk.sh   # pin a specific release
#   NDK_DIR=~/sdk/ndk build/bootstrap-ndk.sh  # choose install location
#
# After it finishes it prints the PATH line to add so cargo can find the
# linker named in .cargo/config.toml (aarch64-linux-android24-clang).

set -euo pipefail

# --- config -----------------------------------------------------------------
REPO_BASE="https://dl.google.com/android/repository"
MANIFEST_URL="$REPO_BASE/repository2-3.xml"
NDK_DIR="${NDK_DIR:-$HOME/android-ndk}"
NDK_VERSION="${NDK_VERSION:-}"   # empty => auto-detect from the manifest

# --- helpers ----------------------------------------------------------------
die() { echo "error: $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# this script only targets a Linux build host (the .zip we pull is *-linux)
[ "$(uname -s)" = "Linux" ] || die "this bootstrap only handles a Linux host; on macOS grab the *-darwin archive instead"

for tool in curl unzip awk grep; do
    have "$tool" || die "missing required tool: $tool"
done

# sha1: coreutils gives sha1sum; fall back to openssl
sha1_of() {
    if have sha1sum; then sha1sum "$1" | awk '{print $1}'
    else openssl dgst -sha1 "$1" | awk '{print $NF}'
    fi
}

# --- 1. resolve which archive to fetch --------------------------------------
manifest="$(mktemp)"
trap 'rm -f "$manifest"' EXIT
echo ">> fetching package manifest"
curl -fsSL "$MANIFEST_URL" -o "$manifest"

if [ -n "$NDK_VERSION" ]; then
    archive="android-ndk-${NDK_VERSION}-linux.zip"
    grep -q "<url>$archive</url>" "$manifest" \
        || die "pinned NDK_VERSION=$NDK_VERSION ($archive) not found in manifest"
else
    # Highest stable android-ndk-r<major><letter?>-linux.zip in the manifest.
    # Excludes beta/rc/canary builds. A missing letter (e.g. r28) is the first
    # release of that major and sorts before r28b/r28c, so we key it as 'a'.
    archive="$(
        grep -oE 'android-ndk-r[0-9]+[a-z]?-linux\.zip' "$manifest" \
            | grep -vE 'beta|rc|canary' \
            | sort -u \
            | awk '{
                  ver = $0
                  sub(/^android-ndk-r/, "", ver); sub(/-linux\.zip$/, "", ver)
                  letter = "a"
                  if (ver ~ /[a-z]$/) { letter = substr(ver, length(ver)); sub(/[a-z]$/, "", ver) }
                  printf "%03d%s\t%s\n", ver, letter, $0
              }' \
            | sort \
            | tail -1 \
            | cut -f2
    )"
    [ -n "$archive" ] || die "could not determine latest NDK from manifest"
fi

echo ">> selected: $archive"

# Pull the expected SHA-1: inside each <complete> block the <checksum> line
# precedes the <url> line, so remember the last checksum seen and print it
# when its url matches our archive.
expected_sha1="$(
    awk -v f="$archive" '
        /<checksum type="sha1">/ { c = $0; gsub(/.*<checksum type="sha1">|<\/checksum>.*/, "", c) }
        $0 ~ ("<url>" f "</url>")  { print c; exit }
    ' "$manifest"
)"
[ -n "$expected_sha1" ] || die "no SHA-1 found for $archive in manifest"

# derive the unpacked dir name, e.g. android-ndk-r28c
ndk_name="${archive%-linux.zip}"
target="$NDK_DIR/$ndk_name"

if [ -d "$target" ]; then
    echo ">> already installed at $target — skipping download"
else
    # --- 2. download & verify ----------------------------------------------
    mkdir -p "$NDK_DIR"
    zip_path="$NDK_DIR/$archive"
    echo ">> downloading $REPO_BASE/$archive"
    curl -fL --progress-bar "$REPO_BASE/$archive" -o "$zip_path"

    echo ">> verifying SHA-1"
    got_sha1="$(sha1_of "$zip_path")"
    [ "$got_sha1" = "$expected_sha1" ] \
        || die "checksum mismatch: expected $expected_sha1, got $got_sha1"

    # --- 3. unpack ---------------------------------------------------------
    echo ">> unzipping into $NDK_DIR"
    unzip -q "$zip_path" -d "$NDK_DIR"
    rm -f "$zip_path"
fi

# --- 4. report what to do next ----------------------------------------------
toolchain_bin="$target/toolchains/llvm/prebuilt/linux-x86_64/bin"
[ -d "$toolchain_bin" ] || die "expected toolchain dir not found: $toolchain_bin"

cat <<EOF

✓ NDK ready: $target

Add the toolchain to your PATH (so cargo finds aarch64-linux-android24-clang):

    export PATH="$toolchain_bin:\$PATH"

Then add the Rust target once and build:

    rustup target add aarch64-linux-android
    cargo build --release --target aarch64-linux-android

Binary: target/aarch64-linux-android/release/localbill
EOF
