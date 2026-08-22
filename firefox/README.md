# WAT on a Firefox fork

The desktop browser, built from a patched Firefox instead of WAT's own engine.
Gecko and its sandbox, WAT's look, and none of the compatibility gap that a
from-scratch engine has on the real web.

**Nothing in this directory has been built.** It is the same kind of scaffolding
as `chromium/`: a version pin, the scripts to fetch, patch and rebase, and an
honest list of what is missing. The machine it was written on has 7.7 GB of disk;
a Firefox checkout is about 40 GB before it compiles. The first person to run
`./fetch.sh && ./build.sh` will find things wrong, and that is expected.

## Why a fork, and not embedding

The desktop app that used to live in `crates/wat-cli` is gone. What replaces it
is a fork, because Gecko offers nothing else: there is no embedding API. Chromium
has CEF, WebKit has WKWebView and WebKitGTK, Servo has an embedding crate — Gecko
has none of those. Mozilla removed the last of them years ago, GeckoView is
Android-only and not a desktop library, and the previous desktop app is what
happens when you write the engine yourself instead.

So "WAT on Firefox" can only mean Firefox with WAT's patches on top, the way
LibreWolf and Waterfox are built. That is a real project with a real cost, and
the cost is the same one `chromium/README.md` describes: a fork's security is its
rebase cadence and nothing else. Firefox ships security fixes every four weeks,
with out-of-band releases for what is already being exploited. A fork four weeks
behind is a browser with four weeks of published, unpatched holes in it.

## What is here

| File | What it is |
| --- | --- |
| `firefox-version.txt` | The release this is pinned to |
| `mozconfig` | The build configuration: official branding off, WAT's on, telemetry and crash reporting out |
| `fetch.sh` | `mach bootstrap`, then a checkout at the pinned version |
| `build.sh` | Apply the patch series, `./mach build`, `./mach package` |
| `rebase.sh` | Move the pin, reapply, and report what rotted |
| `patches/series` | The patch order. Keep it short |

## The patch series

Every patch is a merge conflict waiting to happen, so the series is deliberately
small and each entry has to earn its place.

| Patch | What it does |
| --- | --- |
| `0001-liquid-glass-theme.patch` | A `userChrome`-level theme shipped as a built-in, so the interface matches the Android app |
| `0002-branding.patch` | Names and icons of their own — a fork must not ship as Firefox or use Mozilla's marks |

Neither is written yet. What they have to touch, so the next person is not
guessing:

- **The theme.** `browser/themes/` and `toolkit/themes/`, plus a built-in
  extension under `browser/extensions/`. Firefox's chrome is XUL and CSS, which
  is *far* friendlier to restyling than Chromium's C++ views — this is the one
  place where the Firefox fork is much less work than the Chromium one. The
  Liquid Glass colours can come from `crates/wat-theme/themes/liquid-glass.toml`
  the same way `Glass.kt` does, through a small generator.
- **Branding.** `browser/branding/`, and `MOZ_APP_DISPLAYNAME` in the mozconfig.
- **Telemetry.** `--disable-telemetry` and `--disable-crashreporter` are in the
  mozconfig already; the preferences that ping Mozilla at runtime are a separate
  patch to `browser/app/profile/firefox.js`, and that patch is not written.

## Building

```sh
./firefox/fetch.sh                  # mach bootstrap, then the pinned source
./firefox/build.sh                  # patch, build, package
./firefox/rebase.sh 145.0           # move the pin, reapply, report conflicts
```

A build machine, not a laptop: 16 cores or more, 60 GB of free disk, 16 GB of
RAM. A first build is hours. This is smaller than the Chromium fork's appetite
but it is the same shape of commitment.

CI cannot do this on a hosted GitHub runner — 14 GB of disk — so it needs a
self-hosted runner and a nightly schedule rather than a per-push one.
