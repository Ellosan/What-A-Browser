#!/usr/bin/env bash
#
# Builds the WebView browser.
#
#   ./android-webview/build.sh            debug APK
#   ./android-webview/build.sh release    release APK (unsigned)
#
# Needs the Android SDK and a JDK. No NDK and no Rust toolchain: there is no
# native code here, which is most of why this build is quick.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
profile="${1:-debug}"
case "$profile" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac

task=$([ "$profile" = release ] && echo assembleRelease || echo assembleDebug)
"$here/gradlew" --project-dir "$here" --no-daemon test "$task"

# One APK per architecture, because the bundled tor is 8-9 MB of native code
# and nobody should download four copies of it. The universal one carries all
# four, for anyone who would rather have a single file.
mapfile -t apks < <(find "$here/app/build/outputs/apk/$profile" -name '*.apk' | sort)
test "${#apks[@]}" -gt 0 || { echo "no $profile APK was produced" >&2; exit 1; }
echo "==> ${#apks[@]} APKs"
for apk in "${apks[@]}"; do
    printf '%8s  %s\n' "$(du -h "$apk" | cut -f1)" "$(basename "$apk")"
done
