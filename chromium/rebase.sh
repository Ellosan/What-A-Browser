#!/usr/bin/env bash
#
#   ./rebase.sh 152.0.1234.56
#
# Moves the pin to a new Chromium release and reports which patches no longer
# apply. This is the recurring cost of the fork, and it is not optional: a fork
# that is weeks behind is a browser shipping weeks of published, unpatched
# vulnerabilities. Run it every time upstream cuts a stable release.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${CHROMIUM_ROOT:-$here/checkout}"
src="$root/src"
target="${1:-}"

[ -n "$target" ] || { echo "usage: $0 <chromium-version>" >&2; exit 2; }
[ -d "$src" ] || { echo "no checkout at $src; run ./fetch.sh first" >&2; exit 1; }
export PATH="$root/depot_tools:$PATH"

echo "==> fetching $target"
(cd "$src" && git fetch --tags --depth 1 origin "refs/tags/$target:refs/tags/$target")

echo "==> checking the series against $target before committing to it"
failed=()
while read -r patch; do
    [ -n "$patch" ] || continue
    case "$patch" in \#*) continue ;; esac
    if (cd "$src" && git checkout -q "refs/tags/$target" -- . 2>/dev/null; true) && \
       (cd "$src" && GIT_INDEX_FILE=/dev/null git apply --check "$here/patches/$patch" 2>/dev/null); then
        echo "    ok        $patch"
    else
        echo "    CONFLICT  $patch"
        failed+=("$patch")
    fi
done < "$here/patches/series"

if [ ${#failed[@]} -gt 0 ]; then
    echo
    echo "${#failed[@]} patch(es) need rewriting for $target:"
    printf '    %s\n' "${failed[@]}"
    echo
    echo "The pin has NOT been moved. Fix those patches, then run this again."
    echo "Shipping with a patch dropped silently is how a fork loses a feature"
    echo "without anyone noticing."
    exit 1
fi

echo "$target" > "$here/chromium-version.txt"
echo
echo "pin moved to $target. Now: ./fetch.sh && ./build.sh"
