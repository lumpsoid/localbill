#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

# Load Environment
PROJECT_ROOT="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )/.." && pwd )"
CONFIG_LOADER="$PROJECT_ROOT/scripts/config/config_loader.sh"

if [[ -f "$CONFIG_LOADER" ]]; then
    source "$CONFIG_LOADER"
else
    echo "Error: Config loader not found at $CONFIG_LOADER" >&2
    exit 1
fi

# Path Validation
DATA_DIR="${DATA_DIR:-$PROJECT_ROOT/data}"

if [ ! -d "$DATA_DIR/.git" ]; then
    echo "Error: DATA_DIR ($DATA_DIR) is not a git repository." >&2
    exit 1
fi

check_internet() {
  "$PROJECT_ROOT/scripts/fetch/check_internet.sh"
}

if check_internet; then
    HAS_INTERNET=true
else
    HAS_INTERNET=false
fi

# Prepare Commit Message
COMMENT="${1:-""}"
TIMESTAMP=$(date +"%Y-%m-%d %H:%M:%S")
if [[ -z "$COMMENT" ]]; then
  COMMIT_MSG="Data sync: $TIMESTAMP"
else 
  COMMIT_MSG="Data sync: $TIMESTAMP - $COMMENT"
fi

INTERNET_CHECK="$(check_internet)"

if $HAS_INTERNET; then
    git -C "$DATA_DIR" pull
fi

# Check for changes
if [[ ! -n "$(git -C "$DATA_DIR" status --porcelain)" ]]; then
    echo "No changes detected in $DATA_DIR. Nothing to do."
    exit 1
fi

echo "Changes detected. Staging and committing..."

git -C "$DATA_DIR" add .
git -C "$DATA_DIR" commit -m "$COMMIT_MSG"

if ! $HAS_INTERNET; then
  exit 1
fi

# Push changes to remote origin
# Determine current branch name dynamically
CURRENT_BRANCH=$(git -C "$DATA_DIR" symbolic-ref --short HEAD)

# Verify remote 'origin' exists before pushing
if git -C "$DATA_DIR" remote | grep -q 'origin'; then
    echo "Pushing changes to origin/$CURRENT_BRANCH..."
    git -C "$DATA_DIR" push origin "$CURRENT_BRANCH"
    echo "Success: $COMMIT_MSG and pushed to origin."
else
    echo "Warning: Changes committed locally, but remote 'origin' not found." >&2
fi
