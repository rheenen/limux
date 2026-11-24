#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/release-version.sh
source "$ROOT_DIR/scripts/release-version.sh"
REQUESTED_VERSION="${1:-}"
WORKSPACE_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"

if ! release_version_is_valid "$REQUESTED_VERSION"; then
    echo "ERROR: release version must use x.y.z or x.y.z-suffix (e.g. 0.1.36-zoom.2) without a v prefix: ${REQUESTED_VERSION:-<empty>}" >&2
    exit 1
fi

BASE_VERSION="$(release_base_version "$REQUESTED_VERSION")"
if [ "$BASE_VERSION" != "$WORKSPACE_VERSION" ]; then
    echo "ERROR: release version $REQUESTED_VERSION (base $BASE_VERSION) does not match Cargo workspace version $WORKSPACE_VERSION" >&2
    exit 1
fi

printf '%s\n' "$REQUESTED_VERSION"
