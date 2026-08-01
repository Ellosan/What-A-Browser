#!/usr/bin/env bash
#
# Builds the Android app.
#
#   ./android/build.sh              debug APK for every ABI
#   ./android/build.sh release      release APK (unsigned)
#   ./android/build.sh debug arm64  one ABI, which is much quicker
#
# Needs the Android SDK and NDK, `cargo-ndk`, and the Rust targets:
#
#   cargo install cargo-ndk
#   rustup target add aarch64-linux-android armv7-linux-androideabi \
#       i686-linux-android x86_64-linux-android
#
# ANDROID_HOME and ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) must point at them.

set -euo pipefail

profile="${1:-debug}"
abi_filter="${2:-all}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
android_dir="$root/android"
jni_libs="$android_dir/app/src/main/jniLibs"

case "$profile" in
    debug|release) ;;
    *) echo "usage: $0 [debug|release] [all|arm64|arm|x86|x86_64]" >&2; exit 2 ;;
esac

# Every ABI Google Play accepts. A single one is much quicker to iterate on.
case "$abi_filter" in
    all)    abis=(arm64-v8a armeabi-v7a x86 x86_64) ;;
    arm64)  abis=(arm64-v8a) ;;
    arm)    abis=(armeabi-v7a) ;;
    x86)    abis=(x86) ;;
    x86_64) abis=(x86_64) ;;
    *) echo "unknown ABI: $abi_filter" >&2; exit 2 ;;
esac

if [ -z "${ANDROID_NDK_HOME:-}" ] && [ -n "${ANDROID_NDK_ROOT:-}" ]; then
    export ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
fi
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    echo "set ANDROID_NDK_HOME to your NDK, e.g. \$ANDROID_HOME/ndk/27.0.12077973" >&2
    exit 1
fi

echo "==> building libwat_shell.so for ${abis[*]} ($profile)"
ndk_args=()
for abi in "${abis[@]}"; do
    ndk_args+=(-t "$abi")
done
# `cargo ndk` copies a library into place only when it is newer than what is
# already there. Both profiles write to the same filename, so building debug and
# then release left the debug library sitting in the APK — a release APK with a
# debug build inside it, reported as a success. Clear the destination rather than
# trust it.
for abi in "${abis[@]}"; do
    rm -f "$jni_libs/$abi/libwat_shell.so"
done

# API 24 is the app's minSdk; the NDK needs telling separately.
cargo ndk "${ndk_args[@]}" --platform 24 -o "$jni_libs" \
    build -p wat-shell --lib $([ "$profile" = release ] && echo --release || true)

for abi in "${abis[@]}"; do
    test -f "$jni_libs/$abi/libwat_shell.so" \
        || { echo "cargo ndk produced no library for $abi" >&2; exit 1; }
done

echo "==> assembling the APK"
gradle_task=$([ "$profile" = release ] && echo assembleRelease || echo assembleDebug)
# The wrapper is committed and pins the Gradle version, because the Android
# plugin only works with a matching one: a Gradle picked up from the PATH may be
# too new (9.x rejects what the plugin does) or too old. It is downgraded to a
# warning rather than an error so the script still works without it.
if [ -x "$android_dir/gradlew" ]; then
    gradle_cmd="$android_dir/gradlew"
elif command -v gradle >/dev/null; then
    echo "warning: no ./android/gradlew, falling back to $(gradle --version | awk '/^Gradle/ {print $2}' | head -1) from PATH" >&2
    gradle_cmd="gradle"
else
    echo "no Gradle: ./android/gradlew is missing and none is on the PATH" >&2
    exit 1
fi
(cd "$android_dir" && "$gradle_cmd" --no-daemon "$gradle_task")

# Scoped to the profile that was asked for: searching the whole tree finds the
# other profile's leftovers and reports the wrong file.
apk=$(find "$android_dir/app/build/outputs/apk/$profile" -name '*.apk' | head -1)
test -n "$apk" || { echo "no $profile APK was produced" >&2; exit 1; }
echo "==> $apk"
