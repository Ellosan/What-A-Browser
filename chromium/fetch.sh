#!/usr/bin/env bash
#
# Fetches a Chromium checkout at the pinned version.
#
# Needs tens of gigabytes and a long time. Nothing here is quick, and none of it
# fits on a hosted CI runner.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="$(tr -d '[:space:]' < "$here/chromium-version.txt")"
root="${CHROMIUM_ROOT:-$here/checkout}"

# 40 GB is the source tree alone; the build output is more again.
available=$(df --output=avail -BG "$(dirname "$root")" 2>/dev/null | tail -1 | tr -dc 0-9 || echo 0)
if [ "${available:-0}" -lt 150 ]; then
    echo "warning: ${available}G free where the checkout is going." >&2
    echo "         The source is ~40G and a build needs ~100G more." >&2
    echo "         Set CHROMIUM_ROOT to somewhere with room, or stop now." >&2
    read -r -p "         Continue anyway? [y/N] " reply
    [ "$reply" = y ] || exit 1
fi

mkdir -p "$root"

# depot_tools carries gclient, gn and the rest of the build toolchain.
if [ ! -d "$root/depot_tools" ]; then
    echo "==> depot_tools"
    git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git \
        "$root/depot_tools"
fi
export PATH="$root/depot_tools:$PATH"

if [ ! -d "$root/src" ]; then
    echo "==> fetching Chromium (this takes a long while)"
    # `--nohooks` because the hooks are run below, after the version is pinned:
    # running them twice downloads the toolchain twice.
    (cd "$root" && fetch --nohooks --no-history android)
fi

echo "==> pinning to $version"
(cd "$root/src" && git fetch --tags --depth 1 origin "refs/tags/$version:refs/tags/$version" \
    && git checkout "refs/tags/$version")

# `--with_branch_heads --with_tags` because a release tag's DEPS reference
# branch-head revisions that a default sync does not fetch.
echo "==> syncing dependencies to match"
(cd "$root/src" && gclient sync --with_branch_heads --with_tags --reset --delete_unversioned_trees)

echo "==> installing build dependencies (sudo, Debian/Ubuntu only)"
(cd "$root/src" && ./build/install-build-deps.sh --android --no-prompt) || {
    echo "warning: install-build-deps failed; on a non-Debian host install the" >&2
    echo "         equivalents by hand." >&2
}

(cd "$root/src" && gclient runhooks)

echo
echo "checkout ready at $root/src ($version)"
echo "next: ./build.sh"
