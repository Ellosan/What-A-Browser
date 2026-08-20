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
| **Private windows** | Hiding cat, and hiding lion with Tor bundled in the app — a process each, see below |
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

**Tor is in the app.** Until 0.1.3 this needed Orbot installed, which is a fair
thing to ask of someone who already knows what Orbot is and no thing at all to
ask of everyone else. Desktop Brave does not ask; it carries Tor. So does this:
`libtor.so` — the real tor, from the kmp-tor project's build of the tor source —
ships in the APK and is loaded into the lion window's process. There is nothing
else to install.

It is loaded as a library rather than executed as a binary, which side-steps
Android's ban on executing files outside an app's native library directory.

`ProxyController` — the only way to point a WebView at a proxy — speaks HTTP
proxies and nothing else, while tor speaks SOCKS5. `TorBridge` is the fifty lines
that join them: it accepts `CONNECT host:port` on loopback, opens a SOCKS5 tunnel
through tor, and copies bytes. Two properties matter, and both are tested. It
**only tunnels** — anything that is not a CONNECT is refused, so it cannot be
talked into fetching a URL for anyone. And it **never resolves a name**: the host
goes to tor as a name, so the lookup happens inside the circuit. Resolving here
would put the traffic inside Tor and the DNS queries outside it, naming every
site visited.

It **fails closed**, deliberately, in two places. The proxy override carries no
direct fallback, so if tor stops, pages fail rather than quietly going out over
the ordinary network. And the window loads nothing at all until
`check.torproject.org` has confirmed — through that same proxied WebView, not
through the app's own HTTP client, which would answer for the wrong connection —
that Tor is what it sees. The answer is read with `evaluateJavascript`, which is
the app calling into the page; there is still no JavaScript interface anywhere in
this browser.

**When it fails.** A Tor window is the hardest thing here to diagnose: it runs in
its own process, and until 0.1.4 it blocked screenshots from the moment it opened
— so its own error message could not be photographed, which is a mistake worth
naming. Now `FLAG_SECURE` goes on with the first *page*, not with the window, and
a failure offers "What went wrong": the reason with its exception chain, the
device's architectures, whether this APK actually contains a tor library for it,
whether the WebView supports proxying, the SOCKS and bridge addresses, and the
last forty things tor said — timestamped relative to the window opening rather
than to the clock, since a report gets pasted somewhere. It can be copied or
shared, and it also goes to `adb logcat -s wat-tor`.

The failure worth naming precisely is the one that has nothing to do with tor:
the build ships one APK per architecture, and an APK for the wrong one has no
`libtor.so` in it at all. That is checked before tor is started, and reported as
such rather than as "could not find something".

**It is not the Tor Browser.** Tor Browser's real work is making every user look
identical — fonts, screen size, timing, canvas — and none of that is possible in
a system WebView. This hides *where you are connecting from*, not *who is
connecting*: a site can still tell one visitor from another. The window says so,
once, before the first page.

**What it costs.** Tor is 8–9 MB of native code per architecture, and it is the
whole reason this APK went from 121 KB to 7.9 MB. The build splits per
architecture so nobody downloads four copies, and the library is compressed in
the APK because this is sideloaded rather than delivered by a store. Cold start
is untouched: none of it is loaded, or even class-loaded, unless a Tor window is
opened, and that happens in a different process from the ordinary browser.

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

0.1.3 adds the three things 0.1.2's version was missing, which were the three
that matter most:

- **The edge bends.** A real pane is thick and its edges are curved, so the last
  few millimetres show what is behind them squeezed and pulled inward. `Lens`
  resamples the captured backdrop with a squared falloff towards each edge. This
  is the strongest single cue that a surface is glass; a blur alone reads as
  frosted plastic.
- **The light moves.** `Tilt` reads the accelerometer — gravity's direction in
  the phone's frame *is* the tilt — and slides a specular band across the bar as
  the phone turns. Low-pass filtered, because following an accelerometer exactly
  makes the highlight shake, and stopped in `onPause`, because a sensor left
  registered is a battery complaint nobody can trace back to a highlight. No
  permission is involved: the motion sensors that need one are the step counter
  and the heart rate monitor.
- **It keeps up, and it answers.** Captures are now throttled rather than merely
  deferred, so the backdrop refreshes about eight times a second *during* a
  scroll and once more when it stops, and each new one cross-fades in rather than
  popping. A touch brightens the glass where the finger landed and settles back
  over 450 ms — the event is observed, never consumed, so the buttons underneath
  behave exactly as they did.

What is still missing: the backdrop is a frame or two behind during a fast fling,
and the refraction is a resample rather than true per-pixel optics, so it bends
what is behind the edge without magnifying it. The colours are not guesswork —
`Glass.kt` is generated from the same `liquid-glass.toml` the Rust browser reads.


### The icon

Drawn, not drawn on. `cargo run --release --example app_icon -p wat-paint` renders
every launcher icon with the browser's own rasterizer, from the same palette as
the interface: a gradient tile with the sheen and rim the toolbars have, and a W
built from four rotated rounded bars rather than from a font — so the icon does
not depend on which fonts the machine building it happens to have.

It produces the legacy icon with a margin (the old ones filled every pixel of
their square, which is what Android's lint had been complaining about), a round
variant, and the three layers of an adaptive icon: a full-bleed background, the
mark inside the 66dp safe zone, and a monochrome silhouette for Android 13's
themed icons. Both Android apps in this repository use them.

## Why it launches quickly

- **7.9 MB release APK** per architecture, 8–9 MB of which is the bundled tor.
  Without it the app is still the same 121 KB it was in 0.1.2 — nothing else grew
  — and none of tor is loaded unless a Tor window is opened. No shared library to
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

96 JVM unit tests, all of the security-relevant logic among them: the scheme
policy, download file names, tab order and eviction, the search templates, the
desktop user agent, the Tor check's answer, the SOCKS5 and HTTP CONNECT wire
formats the Tor bridge speaks, the menu's stored layout, the bounds of the blur
and the lens, and the diagnostic report's own limits — that it is bounded, and
that a newline in a value cannot forge a second field in it. They run on every push, before the APK is built, and CI then
checks the built APK asks for `INTERNET` and nothing else, still refuses
cleartext, still gives each private window a process of its own, and still
carries a tor library for every architecture. The last two because both
regressions are invisible from the outside: a private window sharing the ordinary
cookie jar looks identical, and a missing tor library fails only on the phones
nobody testing it happens to hold.

None of it has run on a phone — there is no device or emulator in the build
environment, and no KVM to run one under. What is verified is the tests, the
build, Android lint (0 errors) and what is inside the APK.
