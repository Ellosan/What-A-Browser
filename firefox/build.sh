#!/usr/bin/env bash
#
# Applies the patch series and builds.
#
#   ./firefox/build.sh
#
# Hours, on a machine with 16 cores and 60 GB free.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checkout="$here/checkout"
test -d "$checkout" || { echo "run fetch.sh first" >&2; exit 1; }

cp "$here/mozconfig" "$checkout/mozconfig"

cd "$checkout"
echo "==> applying patches"
while read -r patch; do
    [ -z "$patch" ] && continue
    case "$patch" in \#*) continue ;; esac
    echo "    $patch"
    git apply --check "$here/patches/$patch" || {
        echo "$patch does not apply to $(cat "$here/firefox-version.txt")" >&2
        exit 1
    }
    git apply "$here/patches/$patch"
done < "$here/patches/series"

echo "==> building"
./mach build
./mach package
echo "==> packaged in $checkout/objdir-wat/dist"
