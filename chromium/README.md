# WAT on a Chromium fork

The Android browser, built from a patched Chromium rather than from WAT's own
engine. Chromium's engine and sandbox, WAT's look, and extensions — which Chrome
for Android does not have.

**Nothing in this directory has been built.** It was written against Chromium
151.0.7922.71's real source, read file by file, but the machine it was written on
had 7.7 GB of disk and four cores; a Chromium checkout is 30–40 GB before
compiling and the build wants 100 GB and many more cores than that. Treat the GN
args and the patches as a first draft that has never seen a compiler. The first
person to run `./fetch.sh && ./build.sh` will find things wrong, and that is
expected.

## What you are signing up for

A fork's security is its rebase cadence, and nothing else. Chromium ships
security fixes roughly weekly, many for bugs already being exploited. A fork that
is four weeks behind is a browser with four weeks of published, unpatched
vulnerabilities in it. This is what killed Bromite, and it is not a problem you
can solve by being careful — only by rebasing, forever, on a schedule.

So: `chromium-version.txt` is the pin, `rebase.sh` moves it, and the patch series
is kept deliberately small because every patch is a merge conflict waiting to
happen. Adding a feature to this fork is not free once; it is a tax on every
rebase afterwards.

Extensions cut the other way too. The extension APIs are a large attack surface
that Chrome for Android does not expose, and turning them on is a considered
reduction in the sandbox's guarantees, not a neutral feature flag. Given "make it
secure" was one of the goals here, that tension is worth being explicit about
rather than discovering later.

## Extensions: the flag already exists

The important finding. In `extensions/buildflags/buildflags.gni`:

```gn
enable_extensions = !is_android && !is_ios && !is_castos && !is_fuchsia
enable_desktop_android_extensions = is_desktop_android
```

Extensions are off on Android by construction — but upstream is actively
building Android extension support behind `enable_desktop_android_extensions`,
which their own comment describes as "very much in-development, non-stable, and
likely to crash at any given moment", with a tracking bug at crbug.com/356905053.

This matters a great deal for the size of the job. Kiwi Browser had to patch the
extensions system onto Android from scratch. Riding upstream's flag instead means
the patch is small, and every rebase inherits their progress rather than fighting
it. It also means the feature will be unstable for a while, because upstream says
so.

## The patch series

`patches/series` is the order they apply in. Keep it short.

| Patch | What it does |
| --- | --- |
| `0001-allow-extensions-on-android.patch` | Adds a `wat_android_extensions` GN arg that opts Android into the extensions platform |

Everything else the fork needs is not written yet. Honestly listed, with the
files it has to touch, so the next person is not guessing:

- **Branding.** `chrome/app/theme/`, `chrome/android/java/res/`, and the
  `chrome/app/theme/chromium` → product-name plumbing. A fork must not ship as
  "Chrome" or use Google's marks; the name and icons have to be the fork's own.
- **Liquid Glass.** Chromium's Android UI is Java and Kotlin views, not WAT's
  display list, so *none* of `wat-ui`, `wat-paint` or `wat-theme` transfers. The
  look has to be rebuilt in `chrome/android/java/res/values/` and the toolbar
  classes under `chrome/android/java/src/org/chromium/chrome/browser/toolbar/`.
  This is the largest single piece of work and it is a rewrite, not a port.
- **Google API keys.** A fork has no right to Chrome's. Either register your own
  or build with none, which disables sync, safe browsing lookups and geolocation.
  `use_official_google_api_keys = false` is set in the args for that reason.
- **An extensions UI.** The flag compiles the platform in; installing and
  managing extensions on a phone still needs somewhere to do it from.

## Building

```sh
./chromium/fetch.sh     # depot_tools, then a checkout at the pinned version
./chromium/build.sh     # apply patches, gn gen, autoninja, produce the APK
./chromium/rebase.sh 152.0.1234.56   # move the pin, reapply, report conflicts
```

A build machine, not a laptop: 32 cores or more, 200 GB of free disk, 32 GB of
RAM. A first build is hours. `fetch.sh` alone downloads tens of gigabytes.

CI cannot do this on a hosted GitHub runner — they have 14 GB of disk. It needs a
self-hosted runner, and the run is long enough that per-push builds are not
realistic; nightly is.
