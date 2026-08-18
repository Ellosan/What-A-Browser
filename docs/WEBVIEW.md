# WAT on Android's WebView

The Android browser built on the system WebView: Chromium's engine and sandbox,
WAT's interface, and none of the maintenance of a fork.

This is the third Android option in the repository, and the trade it makes is the
opposite of the other two. `android/` runs WAT's own engine — fastest to launch,
limited web compatibility. `chromium/` forks Chromium — full compatibility and
extensions, at the cost of a permanent rebase duty and a slow launch. This one
takes Chromium's engine without owning it.

## Why the system WebView is more secure than a fork

The instinct is that forking gives more control and therefore more safety. For a
browser it is the other way round.

Android's WebView is Chromium, updated by Google through Play independently of
this app. A security fix lands on the device without the browser being rebuilt,
rebased or even released. A fork gets that fix only when someone merges it, and a
fork four weeks behind is a browser shipping four weeks of published, unpatched
vulnerabilities. Nothing in this app can fall behind, because there is nothing to
keep up to date.

What is given up is control over the engine: no extensions, no engine flags, and
whatever WebView version the device happens to have.

## What is hardened, and why

A `WebView` is configured out of the box to host an app's *own* trusted content.
Pointing it at the open web inverts nearly every default. `SecureWebView.kt` has
the full list with reasons; the ones that matter most:

| Setting | Why |
| --- | --- |
| `allowFileAccess`, `allowContentAccess` off | A page that reaches `file://` or `content://` reads app storage and the user's documents |
| `allowFileAccessFromFileURLs`, `allowUniversalAccessFromFileURLs` off | The classic same-origin escape; both default to true on older platforms |
| `MIXED_CONTENT_NEVER_ALLOW` | The default permits an HTTPS page to pull scripts over HTTP, which is an HTTPS page an attacker on the network controls |
| No `addJavascriptInterface` anywhere | The single most reliable way to turn a WebView into remote code execution. A browser has no reason to bridge web content into the app |
| `onReceivedSslError` always cancels | `handler.proceed()` is the most common critical bug in apps that embed a WebView. There is no "continue anyway" here on purpose — that is the same hole with a consent dialog in front of it |
| Third-party cookies off | Tracking, and a CSRF ingredient |
| Cleartext refused app-wide | `network_security_config.xml`, no per-domain exceptions |
| System CA roots only | User-installed CAs are how middleboxes and malware intercept HTTPS |
| Only `http` and `https` render | `javascript:`, `data:`, `file:`, `content:` and `intent:` are refused, from links *and* from the address bar |
| One permission: `INTERNET` | No location, camera, microphone, contacts or storage. A browser that never asks cannot be tricked into handing them to a page |

The scheme policy is in `UrlResolver.kt`, deliberately free of `android.net.Uri`
so it runs in ordinary JVM tests — a security boundary only ever exercised by
hand on a device is one nobody has checked. Writing those tests immediately found
a real bug: `localhost:8080` parsed "localhost" as a URL scheme.

A typed address is upgraded to `https://`, never `http://`. Guessing cleartext is
how typing an address becomes a downgrade attack.

## The look

`GlassSurface.kt` rebuilds Liquid Glass out of what Android draws cheaply: a
tinted translucent fill, a top-down sheen, and a hairline edge.

The colours are not transcribed. `Glass.kt` is generated from
`crates/wat-theme/themes/liquid-glass.toml` — the same file the Rust browser
reads — by `cargo run --example android_theme -p wat-theme`. Hand-copying is how
two apps meant to look identical quietly stop matching. The generated file is
committed so this build needs no Rust toolchain.

**There is no live backdrop blur, on purpose.** Android has no way to blur live
content behind an ordinary view: `RenderEffect` blurs the view it is set on, and
`Window.setBackgroundBlurRadius` blurs what is behind the *window*, not what is
behind a toolbar inside it. Doing it properly means capturing the WebView to a
bitmap and blurring it every frame, which on a 4 GB phone costs more than the
effect is worth. The glass reads as glass without it.

## Why it launches quickly

- **76 KB release APK**, against 18 MB for the native build. No shared library to
  load, no Rust runtime to start.
- **No Compose, no AppCompat.** Plain `Activity`, plain views, one dependency
  (`androidx.webkit`). Every library linked here is initialised before the first
  frame.
- The WebView itself is often already resident in memory, because other apps use
  it.

None of that is measured on a device — there is no phone here. The APK size is
measured; the launch time is not.

## Building

```sh
./android-webview/build.sh            # tests, then a debug APK
./android-webview/build.sh release    # release APK, unsigned
```

Needs the Android SDK and a JDK. **No NDK and no Rust toolchain**, which is most
of why this build takes a minute rather than an hour.

To regenerate the palette after editing the theme:

```sh
cargo run --example android_theme -p wat-theme -- crates/wat-theme/themes/liquid-glass.toml \
  > android-webview/app/src/main/java/com/whatabrowser/wat/webview/Glass.kt
```

## What is not built yet

- **Tabs.** One WebView, one page. The interface has no tab strip because there
  is nothing behind it yet.
- **Downloads.** `setDownloadListener` is not wired, so a download link does
  nothing.
- **File uploads.** `onShowFileChooser` is not implemented, so `<input type=file>`
  does nothing. Both of these are where a browser usually meets the filesystem,
  and both need care given everything above.
- **Find in page, history, bookmarks, settings.**
