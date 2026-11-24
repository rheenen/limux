#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
VALIDATOR="$ROOT_DIR/scripts/validate-release-version.sh"
# shellcheck source=scripts/release-version.sh
source "$ROOT_DIR/scripts/release-version.sh"
WORKSPACE_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

for valid in "$WORKSPACE_VERSION" "$WORKSPACE_VERSION-zoom.2" "$WORKSPACE_VERSION-rc1"; do
    actual="$($VALIDATOR "$valid")" || fail "rejected valid release version: $valid"
    [ "$actual" = "$valid" ] || fail "validator printed '$actual' for '$valid'"
done

for invalid in "" "v$WORKSPACE_VERSION" "0.1" "next" "$WORKSPACE_VERSION-" \
    "$WORKSPACE_VERSION-zoom..2" "$WORKSPACE_VERSION-zoom_2" "$WORKSPACE_VERSION+zoom"; do
    if "$VALIDATOR" "$invalid" >/dev/null 2>&1; then
        fail "accepted invalid release version: ${invalid:-<empty>}"
    fi
done

for mismatch in "999.999.999" "999.999.999-zoom.1"; do
    if "$VALIDATOR" "$mismatch" >/dev/null 2>&1; then
        fail "accepted version that differs from Cargo.toml: $mismatch"
    fi
done

[ "$(release_base_version 0.1.36)" = "0.1.36" ] || fail "base of 0.1.36"
[ "$(release_base_version 0.1.36-zoom.2)" = "0.1.36" ] || fail "base of 0.1.36-zoom.2"
[ "$(rpm_release 0.1.36)" = "1" ] || fail "rpm release of 0.1.36"
[ "$(rpm_release 0.1.36-zoom.2)" = "1.zoom.2" ] || fail "rpm release of 0.1.36-zoom.2"
[ "$(rpm_file_name 0.1.36 x86_64)" = "limux-0.1.36-1.x86_64.rpm" ] || fail "rpm file name of 0.1.36"
[ "$(rpm_file_name 0.1.36-zoom.2 x86_64)" = "limux-0.1.36-1.zoom.2.x86_64.rpm" ] ||
    fail "rpm file name of 0.1.36-zoom.2"

echo "release version validation: OK"
