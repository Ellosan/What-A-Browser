#!/usr/bin/env bash
#
# Checks that a built APK is the thing this browser claims to be.
#
#   ./android-webview/verify-apk.sh app-universal-release-unsigned.apk
#
# Every one of these is a property that is invisible from the outside once it
# regresses: an app that quietly asks for another permission, a private window
# that quietly shares the ordinary cookie jar, a build that quietly lost tor for
# one architecture. They are checked on the built artifact rather than in the
# source, because the source is not what gets installed.

set -euo pipefail

apk="${1:?usage: verify-apk.sh <apk>}"
test -f "$apk" || { echo "no such APK: $apk" >&2; exit 1; }

# Whichever build-tools the runner has.
aapt2=$(find "${ANDROID_HOME:?ANDROID_HOME is not set}/build-tools" -name aapt2 -type f | sort -V | tail -1)
test -n "$aapt2" || { echo "no aapt2 in $ANDROID_HOME/build-tools" >&2; exit 1; }

fail() { echo "FAIL: $*" >&2; exit 1; }

echo "== $(basename "$apk")"

# One permission, and it is INTERNET. A browser that never asks for location,
# camera, microphone or storage cannot be tricked into handing them to a page.
permissions=$("$aapt2" dump badging "$apk" | grep "^uses-permission" || true)
echo "$permissions"
[ "$(echo "$permissions" | grep -c .)" -eq 1 ] || fail "expected exactly one permission"
echo "$permissions" | grep -q "android.permission.INTERNET" || fail "the one permission should be INTERNET"

manifest=$("$aapt2" dump xmltree --file AndroidManifest.xml "$apk")

# Cleartext stays off: a browser that loads http:// silently is the thing this
# app is meant not to be.
echo "$manifest" | grep -q "usesCleartextTraffic(0x010104ec)=false" || fail "cleartext is not disabled"

# Private windows are private *because* they run in their own process — Android's
# WebView keeps one cookie jar per process. Without this the window still says
# "Hiding cat" while sharing the ordinary cookies.
for process in ':cat' ':lion'; do
    echo "$manifest" | grep -q "process(0x01010011)=\"$process\"" \
        || fail "the private window for $process has no process of its own"
done

# The Tor window's startup provider has to exist and has to be bound to the Tor
# process. Without it, androidx.startup never runs there, kmp-tor never learns
# where its libraries are, and tor reports libtor.so missing while the file sits
# in the app's own library directory. That failure has happened twice; it cannot
# be seen from outside the app, so it is checked here.
echo "$manifest" | grep -q 'name(0x01010003)="[^"]*TorStartupProvider"' \
    || fail "no TorStartupProvider — the Tor process would have no androidx.startup"
echo "$manifest" | grep -A4 'TorStartupProvider' | grep -q 'process(0x01010011)=":lion"' \
    || fail "TorStartupProvider is not bound to the :lion process"

# Tor is carried in the APK rather than asked of another app, so every
# architecture this APK claims to support needs a tor library in it. A missing
# one fails only on the phones nobody testing it happens to hold.
# Listed once and searched in memory. `unzip -l | grep -q` looks right and is
# not: grep stops at the first match, unzip takes the SIGPIPE, and under
# `pipefail` the pipeline reports a failure for a check that actually passed.
listing=$(unzip -l "$apk")
abis=$(grep -oE 'lib/[^/]+/' <<<"$listing" | cut -d/ -f2 | sort -u)
test -n "$abis" || fail "no native libraries at all"
for abi in $abis; do
    grep -q "lib/$abi/libtor.so" <<<"$listing" || fail "no tor library for $abi"
done
case "$(basename "$apk")" in
    *universal*)
        for abi in arm64-v8a armeabi-v7a x86 x86_64; do
            echo "$abis" | grep -qx "$abi" || fail "the universal APK is missing $abi"
        done
        ;;
esac
echo "  architectures: $(echo "$abis" | tr '\n' ' ')"

version=$("$aapt2" dump badging "$apk" | grep -oE "versionName='[^']*'" | cut -d"'" -f2)
echo "  version: $version"
echo "  OK"
