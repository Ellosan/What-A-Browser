#!/usr/bin/env bash
#
# Fetches the Firefox source at the pinned version.
#
# Wants tens of gigabytes of disk and a long time. `mach bootstrap` installs the
# build dependencies for this machine, which is the part that cannot be
# scripted portably — it asks questions, and the answers depend on the host.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="$(tr -d '[:space:]' < "$here/firefox-version.txt")"
checkout="$here/checkout"

command -v git >/dev/null || { echo "git is needed" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is needed" >&2; exit 1; }

if [ ! -d "$checkout" ]; then
    echo "==> cloning firefox (this is tens of gigabytes)"
    git clone --filter=blob:none https://github.com/mozilla-firefox/firefox.git "$checkout"
fi

cd "$checkout"
echo "==> checking out FIREFOX_${version//./_}_RELEASE"
git fetch --tags origin
git checkout "FIREFOX_${version//./_}_RELEASE" 2>/dev/null || {
    echo "no such release tag; the pin in firefox-version.txt may be wrong" >&2
    exit 1
}

echo "==> mach bootstrap"
./mach --no-interactive bootstrap --application-choice browser
echo "==> done: $checkout"
