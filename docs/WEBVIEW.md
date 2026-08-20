# WAT on Android's WebView

The Android browser built on the system WebView: Chromium's engine and sandbox,
WAT's interface, and none of the maintenance of a fork.

This is the third Android option in the repository, and the trade it makes is the
opposite of the other two. `android/` runs WAT's own engine — fastest to launch,
limited web compatibility. `chromium/` forks Chromium — full compatibility and
extensions, at the cost of a permanent rebase duty and a slow launch. This one
takes Chromium's engine without owning it.

## What it does

An everyday browser, in about 2,000 lines of Kotlin:

| | |
| --- | --- |
| **Tabs** | Up to 16, opened from links, `target="_blank"` and the switcher. A link opens next to the page it came from, not at the end of the strip |
| **Address bar** | Search or address, with suggestions from bookmarks and history. Five search engines to choose from, all HTTPS |
| **Bookmarks and history** | SQLite, one row per page, written off the main thread. History can be turned off entirely |
| **Downloads** | Through the system download manager, with the file name sanitised first — see below |
| **File uploads** | `<input type=file>`, through the document picker, which needs no storage permission |
| **Find in page** | With a match counter |
| **Share, copy, long-press menu** | Open in a new tab, copy, share or download a link |
| **Desktop site** | Per tab, by user agent |
| **Fullscreen video** | `onShowCustomView`, so the fullscreen button on video sites works |
| **Page dialogs** | `alert`, `confirm`, `prompt` and "leave this page?", each naming the site that is asking |
| **Settings** | Search engine, home page, desktop sites, history, and clearing cookies, site storage and history |
| **Private windows** | Hiding cat, and hiding lion with Tor — a process each, see below |
| **History on the back button** | Held rather than tapped: the pages behind or ahead, as a list |
| **A menu you arrange** | Which items appear and in what order |

### Tabs on a 4 GB phone

A live `WebView` costs tens of megabytes of renderer state, so sixteen of them is
how a browser gets killed in the background with everything in it. Only the three
most recently used tabs keep a view; the rest are frozen to a saved state and
restored — scroll position and back history included — when they come forward.
`TabList` decides which to freeze, and a tab opened in the background costs
nothing at all until it is looked at.

### Private windows: hiding cat, and hiding lion

Android's WebView keeps one cookie and storage jar per **process**. That single
fact decides the whole design: a private tab living beside an ordinary one would
share its cookies and be private in name only, so a private window here is a
second process — declared in the manifest with `android:process=":cat"` — with a
data directory of its own.

- **Hiding cat** writes nothing down. No history, no suggestions, and no access
  to the bookmarks or history the ordinary window has: the database belongs to
  the ordinary process, and this one neither reads nor writes it. Its data
  directory is deleted the next time a private window starts — on the way in
  rather than on the way out, because a window the system kills never runs its
  own cleanup. `FLAG_SECURE` keeps it out of screenshots and out of the recents
  thumbnail, it never restores a session, and it never writes its tabs into saved
  instance state, which the system may put on disk.
- **Hiding lion** is hiding cat with every request through Tor, in a second
  separate process so that turning Tor on cannot affect the other windows.

Both need Android 9. Before that there is no `setDataDirectorySuffix`, so a
second process cannot have a jar of its own, and the app says exactly that rather
than opening a window that says "private" over the ordinary cookies.

### What hiding lion is, and what it is not

It routes through **Orbot's HTTP proxy** on `127.0.0.1:8118`. `ProxyController`
speaks HTTP proxies only, so Orbot's SOCKS port cannot be used, and Orbot has to
be running with its HTTP proxy switched on.

It **fails closed**, deliberately, in two places. The proxy override carries no
direct fallback, so if Orbot stops, pages fail rather than quietly going out over
the ordinary network. And the window loads nothing at all until
`check.torproject.org` has confirmed — through that same proxied WebView, not
through the app's own HTTP client, which would answer for the wrong connection —
that Tor is what it sees. The answer is read with `evaluateJavascript`, which is
the app calling into the page; there is still no JavaScript interface anywhere in
this browser.

**It is not the Tor Browser.** Tor Browser's real work is making every user look
identical — fonts, screen size, timing, canvas — and none of that is possible in
a system WebView. This hides *where you are connecting from*, not *who is
connecting*: a site can still tell one visitor from another. The window says so,
once, before the first page.

### Downloads and the file name

The name of a downloaded file is chosen by the server, in a header. A server that
sends `Content-Disposition: attachment; filename="../../../shared_prefs/settings.xml"`
is asking the browser to write outside the download directory, and Android's own
`URLUtil.guessFileName` has had that bug more than once. `Downloads.kt` keeps only
the last path segment, strips control characters and the NUL that truncates a path
in every C library underneath, refuses names that are only dots, and there are
tests for each of those with the hostile header written out.

Files go to the app's own external files directory rather than the shared
Downloads folder, because writing to the shared one needs
`WRITE_EXTERNAL_STORAGE` on Android 9 and below — a permission this browser does
not ask for and will not start asking for to save a PDF. They still appear in the
system's Downloads list.

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

`GlassBar` is a pane of glass with the page showing through it, and as of 0.1.2
that is meant literally rather than as a description of a tint.

`Backdrop` captures the strip of page behind each bar into a bitmap **at an
eighth scale**, blurs it, lifts its saturation, and hands it to the bar to draw
as its own backdrop. An eighth scale is not a compromise: a blur throws that
detail away regardless, so a phone-width bar becomes about 135 pixels across
before any work happens. Captures are debounced to after a scroll stops, so the
expensive part never lands on a frame that has to be quick — and `Backdrop`
times itself, switching off for the session after three captures over half a
frame. A phone slow enough to notice this is a phone that should not be paying
for it.

Over the backdrop, the things that make glass read as a surface rather than as a
tinted hole: vibrancy (colour lifted back up after the blur averaged it towards
grey), the tint, a sheen that is bright along the top and gone by a third of the
way down, a lit lower edge, a rim that is brightest at the top and fades around
the sides rather than a stroke of one flat colour, and a soft shadow underneath.
The corners are `GlassShape`'s continuous curve — two cubics per corner, starting
about 1.53 radii along the edge — not a `cornerRadius`, because a circular arc
joins a straight edge with a jump in curvature and the eye reads that jump as a
pinched corner.

The blur is in `Blur.kt`, in plain Kotlin over an `IntArray`, for two reasons:
`RenderEffect` is Android 12 and later and this build starts at 7, and a blur is
arithmetic, so it can be tested off a device. That matters — a blur that reads a
row past the end of its buffer does not look wrong, it crashes.

**On copying Apple's.** This is a look-alike built from scratch out of gradients,
a blur and a curve. Apple's implementation is not published, and could not be
lifted into an Android view if it were; what is copied is the *look* — which
parts are bright, where the light goes, how the corners turn — and the parts
Android cannot do at all are named below rather than glossed over.

What is still missing against the real thing: the backdrop refreshes when the
page settles rather than every frame, so during a scroll the glass is showing a
slightly stale blur; there is no edge lensing, where the rim bends and magnifies
what is behind it; and specular highlights do not move with the phone, because
nothing here reads the gyroscope. The colours themselves are not guesswork —
`Glass.kt` is generated from the same `liquid-glass.toml` the Rust browser reads.

## Why it launches quickly

- **121 KB release APK**, against 18 MB for the native build. No shared library to
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

- **Session restore across a cold start.** Tabs survive rotation and being killed
  in the background, but closing the app forgets them. The back history is kept
  for the tab in front only — every tab's history would cross the 1 MB limit on
  saved state and crash the browser it was meant to protect.
- **`blob:` and `data:` downloads.** These are generated inside the page and the
  system download manager can only fetch over the network. An export button that
  builds a file in JavaScript will not save.
- **Reader mode, translation, autofill, sync, extensions.** Extensions are what
  `chromium/` is for, and the reason that directory exists at all.

## Testing

64 JVM unit tests, all of the security-relevant logic among them: the scheme
policy, download file names, tab order and eviction, the search templates, the
desktop user agent, the Tor check's answer, the menu's stored layout and the blur
kernel's bounds. They run on every push, before the APK is built, and CI then
checks the built APK asks for `INTERNET` and nothing else, still refuses
cleartext, and still gives each private window a process of its own — the last
one because a private window that quietly started sharing the ordinary cookie jar
would look and behave exactly the same.

None of it has run on a phone — there is no device or emulator in the build
environment, and no KVM to run one under. What is verified is the tests, the
build, Android lint (0 errors) and what is inside the APK.
