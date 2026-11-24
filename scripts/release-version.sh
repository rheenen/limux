# shellcheck shell=bash
# Release version helpers. Source this file; do not execute it.
#
# A release version is the Cargo workspace version (x.y.z), optionally
# followed by a pre-release style suffix for fork builds, e.g.
# "0.1.36-zoom.2". The tag is "v" + the release version.
#
# - The base version (x.y.z) must equal the Cargo workspace version; it is
#   what the binaries report via `limux --version`.
# - Artifact file names (tarball, deb, AppImage) carry the full version.
# - The Debian Version field uses the full version: the suffix becomes the
#   Debian revision, so 0.1.36-zoom.2 sorts after a plain 0.1.36.
# - RPM forbids "-" in Version, so RPM gets Version=x.y.z and
#   Release=1.<suffix> (or 1 without a suffix). That also sorts after the
#   plain upstream 0.1.36-1.

RELEASE_VERSION_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?$'

release_version_is_valid() {
    [[ "$1" =~ $RELEASE_VERSION_PATTERN ]]
}

release_base_version() {
    printf '%s\n' "${1%%-*}"
}

release_version_suffix() {
    case "$1" in
        *-*) printf '%s\n' "${1#*-}" ;;
        *) printf '\n' ;;
    esac
}

rpm_release() {
    local suffix
    suffix="$(release_version_suffix "$1")"
    if [ -n "$suffix" ]; then
        printf '1.%s\n' "$suffix"
    else
        printf '1\n'
    fi
}

# rpm_file_name <release-version> <rpm-arch>
rpm_file_name() {
    printf 'limux-%s-%s.%s.rpm\n' "$(release_base_version "$1")" "$(rpm_release "$1")" "$2"
}
