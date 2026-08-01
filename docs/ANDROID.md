# WAT on Android

The Android app is the same browser as the desktop one. There is no Java beyond
a manifest: `NativeActivity` loads `libwat_shell.so`, hands it a window and an
event queue, and everything from the HTML parser to the rasterizer is the Rust
in `crates/`.

That is the whole point of the seam. `wat-shell::browser` has no windowing
dependency, so the platform layer is one file — and Android needed four things
from it, not a port.

## What Android needed

**Device pixels.** A phone reports a scale factor of 2.5 to 3.5. The window used
to treat those as CSS pixels, which drew the entire interface at a third of its
intended size. Now the chrome and the page lay out in CSS pixels and the
finished display list is scaled at the last moment, so text is rasterized at the
size it is actually drawn rather than magnified. `wat shot --scale 3` does the
same thing headlessly, which is how it is checked.

**Touch.** A finger is not a mouse: it has no hover, and whether it is a tap or
a scroll is only known when it lifts. A touch is buffered — pressing is shown
straight away so a button lights up, dragging past 12 CSS pixels turns into a
scroll and cancels the press, and only a finger that stayed put delivers a tap,
at the point it went down rather than where it drifted to. All of that lives in
`Browser`, so it is tested without a device.

**The back gesture.** It arrives as a key. It goes back in history, and only
leaves the app when there is nowhere left to go.

**Fonts.** `fontdb::load_system_fonts` has branches for Windows, macOS, Redox
and Linux, and Android matches none of them — so the font database came up empty
and every glyph fell back to synthetic metrics. `wat-text` now loads
`/system/fonts` and the Treble partitions itself, and names Roboto as the
sans-serif family.

## Building it

You need the Android SDK and NDK, `cargo-ndk`, and the Rust targets:

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi \
    i686-linux-android x86_64-linux-android

export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
```

Then:

```sh
./android/build.sh                  # debug APK, every ABI
./android/build.sh debug arm64      # one ABI, much quicker to iterate on
./android/build.sh release          # release APK, unsigned
```

The APK lands in `android/app/build/outputs/apk/`. Install and watch it:

```sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s WAT
```

`build.sh` is two steps you can also run by hand: `cargo ndk` cross-compiles
`libwat_shell.so` into `android/app/src/main/jniLibs/<abi>/`, and Gradle packages
that directory into an APK.

## Signing a release

The release APK the workflow builds is **unsigned**, because the signing key is
not something to keep in a repository. To sign one yourself:

```sh
keytool -genkey -v -keystore wat.jks -keyalg RSA -keysize 2048 \
    -validity 10000 -alias wat

$ANDROID_HOME/build-tools/34.0.0/apksigner sign \
    --ks wat.jks --out wat-signed.apk \
    android/app/build/outputs/apk/release/app-release-unsigned.apk
```

## What it is and is not

It runs: the whole engine, the mobile chrome layout, touch scrolling and
tapping, the back gesture, JavaScript, and the Liquid Glass theme with real
backdrop blur — on the CPU, since there is no GPU path.

It does not have: a soft keyboard (the address bar takes hardware key events but
does not raise the on-screen one), fling or momentum scrolling, pinch zoom,
insets for the status bar and the notch, a share sheet, downloads, or state
saved across a process death. Rendering is single-threaded software
rasterization, so a complex page is slower than a phone browser you are used to.

**This has not been run on a physical device.** It compiles for all four ABIs,
the shared library exports `android_main` and `ANativeActivity_onCreate`, and
the APK assembles and installs — but nobody has held it. Treat the first run as
the test it has not had.
