#!/usr/bin/env bash
#
# Moves the pin to a newer Firefox and reports what rotted.
#
#   ./firefox/rebase.sh 146.0
#
# This is the job. A fork's security is its rebase cadence and nothing else:
# Firefox ships fixes every four weeks, and a fork that is behind is a browser
# with published holes in it. The pin does not move unless every patch still
# applies, so a rebase that needs work fails here rather than silently shipping.

set -euo pipefail

version="${1:?usage: rebase.sh <version>, e.g. 146.0}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checkout="$here/checkout"
test -d "$checkout" || { echo "run fetch.sh first" >&2; exit 1; }

cd "$checkout"
git fetch --tags origin
tag="FIREFOX_${version//./_}_RELEASE"
git rev-parse "$tag" >/dev/null 2>&1 || { echo "no tag $tag" >&2; exit 1; }
git checkout "$tag"

rotted=0
while read -r patch; do
    [ -z "$patch" ] && continue
    case "$patch" in \#*) continue ;; esac
    if git apply --check "$here/patches/$patch" 2>/dev/null; then
        echo "ok      $patch"
    else
        echo "ROTTED  $patch"
        rotted=$((rotted + 1))
    fi
done < "$here/patches/series"

if [ "$rotted" -gt 0 ]; then
    echo
    echo "$rotted patch(es) need updating; the pin has NOT been moved." >&2
    exit 1
fi

echo "$version" > "$here/firefox-version.txt"
echo "pin moved to $version"
