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

apk=$(find "$here/app/build/outputs/apk/$profile" -name '*.apk' | head -1)
test -n "$apk" || { echo "no $profile APK was produced" >&2; exit 1; }
echo "==> $apk"
ls -lh "$apk"
