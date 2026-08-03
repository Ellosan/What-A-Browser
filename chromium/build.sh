#!/usr/bin/env bash
#
# Applies the patch series and builds the Android APK.
#
#   ./build.sh              arm64, release
#   ./build.sh arm          32-bit ARM
#
# A build machine, not a laptop: 32+ cores, 200 GB free, 32 GB RAM. Hours.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${CHROMIUM_ROOT:-$here/checkout}"
src="$root/src"
version="$(tr -d '[:space:]' < "$here/chromium-version.txt")"

[ -d "$src" ] || { echo "no checkout at $src; run ./fetch.sh first" >&2; exit 1; }
export PATH="$root/depot_tools:$PATH"

case "${1:-arm64}" in
    arm64) cpu=arm64 ;;
    arm)   cpu=arm ;;
    x64)   cpu=x64 ;;
    *) echo "usage: $0 [arm64|arm|x64]" >&2; exit 2 ;;
esac
out="out/wat-$cpu"

# The tree must be at the pinned version, or the patches are being applied to
# something they were not written against and the build is a lie.
actual="$(cd "$src" && git describe --tags --exact-match 2>/dev/null || echo unknown)"
if [ "$actual" != "$version" ]; then
    echo "checkout is at '$actual', pinned version is '$version'." >&2
    echo "Run ./fetch.sh, or ./rebase.sh to move the pin deliberately." >&2
    exit 1
fi

# Start from clean upstream every time. Applying a series twice, or onto a tree
# someone has poked at by hand, produces failures that look like patch rot but
# are not.
echo "==> resetting to $version"
(cd "$src" && git checkout -- . && git clean -fd -- build/config/wat.gni 2>/dev/null || true)

echo "==> applying the patch series"
while read -r patch; do
    [ -n "$patch" ] || continue
    case "$patch" in \#*) continue ;; esac
    echo "    $patch"
    # `--3way` so a patch that has rotted reports a conflict to resolve rather
    # than just refusing, which is the common case after a rebase.
    (cd "$src" && git apply --3way "$here/patches/$patch") || {
        echo >&2
        echo "$patch did not apply to $version." >&2
        echo "That is the rebase tax. Fix the patch in chromium/patches/ and" >&2
        echo "re-run; do not edit the checkout and move on." >&2
        exit 1
    }
done < "$here/patches/series"

echo "==> gn gen $out"
args="$(cat "$here/args/android-arm64.gn")"
# The args file names arm64; anything else overrides it on the last line, since
# GN takes the last assignment.
[ "$cpu" = arm64 ] || args="$args
target_cpu = \"$cpu\""
mkdir -p "$src/$out"
printf '%s\n' "$args" > "$src/$out/args.gn"
(cd "$src" && gn gen "$out")

echo "==> building (hours)"
(cd "$src" && autoninja -C "$out" chrome_public_apk)

apk="$src/$out/apks/ChromePublic.apk"
if [ -f "$apk" ]; then
    echo
    echo "APK: $apk"
    ls -lh "$apk"
else
    echo "build finished but $apk is not there; check the ninja output" >&2
    exit 1
fi
